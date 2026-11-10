//! Registro de Windows nativo y respaldos.
//! Los respaldos usan el mismo formato y carpeta que la app Electron
//! (%ProgramData%\BotoxOptimizer\backup\<id>.json), así los que ya existen se pueden revertir desde acá.
use serde_json::{json, Value};
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
    // BX_BACKUP_DIR: otra carpeta, para los tests.
    if let Ok(d) = std::env::var("BX_BACKUP_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into())).join("BotoxOptimizer").join("backup")
}
pub fn backup_path(id: &str) -> PathBuf { backup_dir().join(format!("{id}.json")) }

/// Guarda el respaldo solo si no existe (nunca pisa los valores originales). Devuelve true si lo creó.
pub fn save_backup(id: &str, items: &Value) -> Result<bool, String> {
    let f = backup_path(id);
    if f.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(backup_dir()).map_err(|e| format!("no se pudo crear la carpeta de respaldos: {e}"))?;
    // Con BOM, igual que PowerShell 5.1, para que la app Electron también lo lea bien.
    let text = format!("\u{feff}{}", serde_json::to_string_pretty(items).unwrap());
    std::fs::write(&f, text).map_err(|e| format!("no se pudo guardar el respaldo: {e}"))?;
    Ok(true)
}

/// Lee y borra el respaldo. Siempre devuelve una lista.
pub fn take_backup(id: &str) -> Option<Vec<Value>> {
    let f = backup_path(id);
    let text = std::fs::read_to_string(&f).ok()?;
    let _ = std::fs::remove_file(&f);
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok()? {
        Value::Array(a) => Some(a),
        Value::Null => Some(vec![]),
        one => Some(vec![one]),
    }
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
