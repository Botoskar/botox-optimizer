//! Registro de Windows nativo y respaldos.
//! Los respaldos usan el mismo formato y carpeta que la app Electron
//! (%ProgramData%\BotoxOptimizer\backup\<id>.json), así los que ya existen se pueden revertir desde acá.
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use winreg::enums::*;
use winreg::{RegKey, RegValue};

fn split(p: &str) -> Option<(RegKey, &str)> {
    let (hive, sub) = p.split_once(":\\")?;
    let root = match hive {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        _ => return None,
    };
    Some((RegKey::predef(root), sub))
}

pub fn key_exists(p: &str) -> bool { split(p).is_some_and(|(r, s)| r.open_subkey(s).is_ok()) }

fn utf16(bytes: &[u8]) -> Vec<u16> { bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() }

/// Lee un valor. Devuelve (valor, tipo) con los nombres de tipo de PowerShell, o None si no existe.
pub fn get(p: &str, n: &str) -> Option<(Value, String)> {
    let (root, sub) = split(p)?;
    let rv = root.open_subkey(sub).ok()?.get_raw_value(n).ok()?;
    let b: &[u8] = &rv.bytes;
    Some(match rv.vtype {
        REG_DWORD if b.len() >= 4 => (json!(i32::from_le_bytes([b[0], b[1], b[2], b[3]])), "DWord".into()),
        REG_QWORD if b.len() >= 8 => (json!(i64::from_le_bytes(b[..8].try_into().unwrap())), "QWord".into()),
        REG_SZ | REG_EXPAND_SZ => {
            let w = utf16(b);
            let s = String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())]);
            (json!(s), if rv.vtype == REG_SZ { "String" } else { "ExpandString" }.into())
        }
        REG_MULTI_SZ => {
            let w = utf16(b);
            let parts: Vec<String> = w.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect();
            (json!(parts), "MultiString".into())
        }
        _ => (json!(b), "Binary".into()),
    })
}

/// Texto de un valor como lo compara PowerShell: números en decimal, binarios separados por espacio.
pub fn fmt(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(fmt).collect::<Vec<_>>().join(" "),
        other => other.to_string(),
    }
}

pub fn set(p: &str, n: &str, v: &Value, t: &str) -> Result<(), String> {
    let wide = |s: &str| s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    let (bytes, vtype) = match t {
        "DWord" => ((v.as_i64().ok_or("valor DWord inválido")? as i32).to_le_bytes().to_vec(), REG_DWORD),
        "QWord" => (v.as_i64().ok_or("valor QWord inválido")?.to_le_bytes().to_vec(), REG_QWORD),
        "String" => (wide(&fmt(v)), REG_SZ),
        "ExpandString" => (wide(&fmt(v)), REG_EXPAND_SZ),
        "MultiString" => {
            let mut b: Vec<u8> = v.as_array().map(|a| a.iter().flat_map(|s| wide(&fmt(s))).collect()).unwrap_or_default();
            b.extend([0, 0]);
            (b, REG_MULTI_SZ)
        }
        "Binary" => (v.as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0) as u8).collect()).unwrap_or_default(), REG_BINARY),
        other => return Err(format!("tipo de registro no soportado: {other}")),
    };
    let (root, sub) = split(p).ok_or_else(|| format!("ruta de registro inválida: {p}"))?;
    let (key, _) = root.create_subkey(sub).map_err(|e| format!("{p}: {e}"))?;
    key.set_raw_value(n, &RegValue { bytes: bytes.into(), vtype }).map_err(|e| format!("{p}\\{n}: {e}"))
}

pub fn del(p: &str, n: &str) {
    if let Some((root, sub)) = split(p) {
        if let Ok(k) = root.open_subkey_with_flags(sub, KEY_SET_VALUE) {
            let _ = k.delete_value(n);
        }
    }
}

/// `HKLM:\SYSTEM\...` → `HKLM\SYSTEM\...` para mostrar en la consola.
pub fn short(p: &str) -> String { p.replacen(":\\", "\\", 1) }

/// Si la entrada es el arranque de un servicio, devuelve su nombre.
pub fn service_of<'a>(p: &'a str, n: &str) -> Option<&'a str> {
    if n != "Start" {
        return None;
    }
    let (_, name) = p.rsplit_once("\\CurrentControlSet\\Services\\")?;
    (!name.is_empty() && !name.contains('\\')).then_some(name)
}

// ── Respaldos ──

pub fn backup_dir() -> PathBuf {
    // BX_BACKUP_DIR solo existe en builds de prueba. En release la carpeta es siempre la de Windows.
    #[cfg(debug_assertions)]
    if let Ok(d) = std::env::var("BX_BACKUP_DIR") {
        return PathBuf::from(d);
    }
    crate::win::program_data().join("BotoxOptimizer").join("backup")
}
pub fn backup_path(id: &str) -> PathBuf { backup_dir().join(format!("{id}.json")) }

/// Rechaza enlaces y, con la app elevada, todo lo que no sea de SYSTEM/Administradores: ProgramData deja que un
/// usuario normal cree carpetas y archivos ahí, y un respaldo plantado se aplicaría como administrador.
fn guard(p: &std::path::Path) -> Result<(), String> {
    if crate::engine::is_admin() {
        crate::win::require_admin_owned(p)
    } else if crate::win::is_reparse_point(p) {
        Err(format!("{} es un enlace/reparse point: no se usa para respaldos", p.display()))
    } else {
        Ok(())
    }
}

/// Crea la carpeta y, con la app elevada, exige que sea de un administrador y la deja solo para SYSTEM y Administradores.
fn ensure_dir() -> Result<(), String> {
    let dir = backup_dir();
    let checks = match dir.parent() {
        Some(parent) => vec![parent.to_path_buf(), dir.clone()],
        None => vec![dir.clone()],
    };
    // Un junction/symlink plantado en ProgramData redirigiría escrituras elevadas a otra carpeta.
    for p in checks.iter().filter(|p| p.exists()) {
        guard(p)?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear la carpeta de respaldos: {e}"))?;
    for p in &checks {
        guard(p)?;
    }
    if crate::engine::is_admin() {
        crate::win::harden_dir(&dir)?;
    }
    Ok(())
}

/// Guarda el respaldo solo si no existe (nunca pisa los valores originales). Devuelve true si lo creó.
pub fn save_backup(id: &str, items: &Value) -> Result<bool, String> {
    let f = backup_path(id);
    if f.exists() || crate::win::is_reparse_point(&f) {
        // Uno que ya existe tiene que ser legítimo: si lo plantó otro usuario, no se toma como "los valores originales".
        guard(&f)?;
        return Ok(false);
    }
    ensure_dir()?;
    let text = format!("\u{feff}{}", serde_json::to_string_pretty(items).unwrap());
    // create_new: si otro proceso (o un archivo plantado) ya lo creó, no se escribe encima.
    let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&f) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(format!("no se pudo guardar el respaldo: {e}")),
    };
    if let Err(e) = file.write_all(text.as_bytes()) {
        let _ = std::fs::remove_file(&f);
        return Err(format!("no se pudo guardar el respaldo: {e}"));
    }
    if let Err(e) = file.sync_all() {
        let _ = std::fs::remove_file(&f);
        return Err(format!("no se pudo guardar el respaldo: {e}"));
    }
    Ok(true)
}

/// Lee el respaldo sin borrarlo. Si está corrupto devuelve Err y lo deja en disco para poder revisarlo.
pub fn read_backup(id: &str) -> Result<Option<Vec<Value>>, String> {
    let f = backup_path(id);
    if !f.exists() && !crate::win::is_reparse_point(&f) {
        return Ok(None);
    }
    // Carpeta y archivo: con la app elevada, solo se aplica un respaldo escrito por un administrador.
    if let Some(dir) = f.parent() {
        guard(dir)?;
    }
    guard(&f)?;
    let text = match std::fs::read_to_string(&f) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("no se pudo leer {}: {e}", f.display())),
    };
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("el respaldo {} está dañado: {e}", f.display()))?;
    Ok(Some(match value {
        Value::Array(a) => a,
        Value::Null => vec![],
        one => vec![one],
    }))
}

/// Borra el respaldo recién cuando la reversión terminó bien.
pub fn remove_backup(id: &str) {
    let _ = std::fs::remove_file(backup_path(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_como_powershell() {
        assert_eq!(fmt(&json!(-1)), "-1");
        assert_eq!(fmt(&json!("506")), "506");
        assert_eq!(fmt(&json!([144, 18, 3])), "144 18 3");
    }

    #[test]
    fn detecta_servicios() {
        assert_eq!(service_of("HKLM:\\SYSTEM\\CurrentControlSet\\Services\\DiagTrack", "Start"), Some("DiagTrack"));
        assert_eq!(service_of("HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters", "Start"), None);
        assert_eq!(service_of("HKLM:\\SYSTEM\\CurrentControlSet\\Services\\DiagTrack", "Otro"), None);
    }

    #[test]
    fn lee_un_valor_real() {
        // Existe en todo Windows y es de solo lectura para la prueba.
        let (v, t) = get("HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentBuild").expect("CurrentBuild");
        assert_eq!(t, "String");
        assert!(fmt(&v).parse::<u32>().unwrap() > 10000);
        assert!(get("HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "NoExisteBotox").is_none());
    }
}
