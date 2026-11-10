//! Actualización desde GitHub Releases.
//! Las versiones se publican en un repositorio público que solo trae el .exe (el código es privado), así la app
//! no necesita ninguna clave. Antes de instalar se verifica el SHA256 que figura en la descripción de la versión.
use crate::engine::{run_ps, Log};
use serde_json::Value;
use std::path::PathBuf;

const REPO: &str = "Botoskar/botox-optimizer-releases";
const ASSET: &str = "BotoxOptimizer.exe";

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// Solo para pruebas: BX_UPDATE_API puede apuntar a un servidor de esta misma PC. Cualquier otro valor se ignora.
fn local(url: &str) -> bool {
    // Después del nombre tiene que venir el puerto o la ruta: "127.0.0.1.evil.com" no vale.
    ["http://127.0.0.1", "http://localhost"].iter().any(|h| url.strip_prefix(h).is_some_and(|r| r.is_empty() || r.starts_with([':', '/'])))
}

fn api_url() -> String {
    match std::env::var("BX_UPDATE_API") {
        Ok(u) if local(&u) => u,
        _ => format!("https://api.github.com/repos/{REPO}/releases/latest"),
    }
}

/// "v2.10.1" → [2, 10, 1]. Se completa con ceros para que 2.1 y 2.1.0 sean iguales.
fn parse_ver(v: &str) -> Option<[u32; 3]> {
    let mut out = [0; 3];
    for (i, p) in v.trim().trim_start_matches('v').split('.').enumerate() {
        *out.get_mut(i)? = p.parse().ok()?;
    }
    Some(out)
}

pub fn is_newer(remote: &str, local: &str) -> bool { matches!((parse_ver(remote), parse_ver(local)), (Some(r), Some(l)) if r > l) }

/// Lee la versión, el archivo y el hash de la respuesta de GitHub. None si falta algo o algo no es de fiar.
pub fn parse(json: &Value) -> Option<Update> {
    let version = json["tag_name"].as_str()?.trim_start_matches('v').to_string();
    let asset = json["assets"].as_array()?.iter().find(|a| a["name"] == ASSET)?;
    let url = asset["browser_download_url"].as_str()?.to_string();
    // Solo se baja del repo de versiones (o de un servidor local en las pruebas).
    if !url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) && !local(&url) {
        return None;
    }
    // El hash va en la descripción: "SHA256: <64 caracteres hexadecimales>".
    let body = json["body"].as_str()?;
    let sha256 = body.split_whitespace().skip_while(|w| !w.eq_ignore_ascii_case("sha256:")).nth(1)?.trim_matches('`').to_ascii_lowercase();
    (sha256.len() == 64 && sha256.bytes().all(|c| c.is_ascii_hexdigit())).then_some(Update { version, url, sha256 })
}

/// Pregunta a GitHub cuál es la última versión. Ok(None) = ya estás al día.
pub fn check() -> Result<Option<Update>, String> {
    let ps = format!(
        "$ProgressPreference = 'SilentlyContinue'\n[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12\nInvoke-RestMethod -UseBasicParsing -TimeoutSec 15 -Headers @{{ 'User-Agent' = 'BotoxOptimizer' }} '{}' | ConvertTo-Json -Depth 6 -Compress",
        api_url()
    );
    let o = run_ps("", &ps, &Log(None));
    if !o.ok {
        return Err(o.err);
    }
    let json: Value = serde_json::from_str(o.out.trim()).map_err(|e| e.to_string())?;
    Ok(parse(&json).filter(|u| is_newer(&u.version, env!("CARGO_PKG_VERSION"))))
}

fn quote(p: &std::path::Path) -> String { p.display().to_string().replace('\'', "''") }

fn side(exe: &std::path::Path, tag: &str) -> PathBuf { exe.with_extension(format!("{tag}.exe")) }

/// Borra la copia anterior que deja una actualización (no se puede mientras el programa viejo sigue abierto).
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(side(&exe, "old"));
        let _ = std::fs::remove_file(side(&exe, "nuevo"));
    }
}

/// Baja el .exe, comprueba el hash y lo pone en lugar del actual. El actual queda como "….old.exe" por si algo falla.
/// Windows deja renombrar un programa mientras corre, por eso no hace falta cerrarlo antes.
pub fn install(u: &Update) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (new, old) = (side(&exe, "nuevo"), side(&exe, "old"));
    let ps = format!(
        "$ProgressPreference = 'SilentlyContinue'\n[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12\nInvoke-WebRequest -UseBasicParsing -TimeoutSec 180 -Headers @{{ 'User-Agent' = 'BotoxOptimizer' }} -Uri '{}' -OutFile '{}'\n(Get-FileHash -Algorithm SHA256 -LiteralPath '{}').Hash",
        u.url, quote(&new), quote(&new)
    );
    let o = run_ps("", &ps, &Log(None));
    let discard = |why: String| { let _ = std::fs::remove_file(&new); Err(why) };
    if !o.ok {
        return discard(format!("No se pudo descargar: {}", o.err));
    }
    if !o.out.trim().eq_ignore_ascii_case(&u.sha256) {
        return discard("El archivo descargado no coincide con el hash publicado. No se instaló nada.".into());
    }
    let head = std::fs::read(&new).map(|b| (b.len(), b.starts_with(b"MZ"))).unwrap_or((0, false));
    if head.0 < 1_000_000 || !head.1 {
        return discard("El archivo descargado no es un programa válido. No se instaló nada.".into());
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("No se pudo reemplazar el programa: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // vuelve atrás
        return Err(format!("No se pudo instalar la versión nueva: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "2596c454190e6c734aacaE3a2f6285928863a04486b889a16e025a53bff0f24c";

    fn release(url: &str, body: &str) -> Value {
        json!({ "tag_name": "v2.1.0", "body": body, "assets": [{ "name": "otro.zip", "browser_download_url": "https://x" }, { "name": ASSET, "browser_download_url": url }] })
    }

    #[test]
    fn compara_versiones_como_numeros() {
        assert!(is_newer("2.1.0", "2.0.0") && is_newer("v2.10.0", "2.9.9") && is_newer("3", "2.9.9"));
        assert!(!is_newer("2.0.0", "2.0.0") && !is_newer("2.1", "2.1.0") && !is_newer("2.0.9", "2.1.0"));
        assert!(!is_newer("basura", "2.0.0") && !is_newer("2.1.0-beta", "2.0.0") && !is_newer("1.2.3.4", "1.0.0"));
    }

    #[test]
    fn lee_la_version_y_el_hash() {
        let url = format!("https://github.com/{REPO}/releases/download/v2.1.0/{ASSET}");
        let u = parse(&release(&url, &format!("Novedades.\n\nSHA256: `{SHA}`\n"))).unwrap();
        assert_eq!((u.version.as_str(), u.url.as_str(), u.sha256.as_str()), ("2.1.0", url.as_str(), SHA.to_ascii_lowercase().as_str()));
    }

    #[test]
    fn rechaza_lo_que_no_es_de_fiar() {
        let bueno = format!("https://github.com/{REPO}/releases/download/v2.1.0/{ASSET}");
        let body = format!("SHA256: {SHA}");
        assert!(parse(&release("https://github.com/otro/repo/releases/download/v1/BotoxOptimizer.exe", &body)).is_none(), "otro repo");
        assert!(parse(&release("https://sitio-malo.com/BotoxOptimizer.exe", &body)).is_none(), "otro dominio");
        assert!(parse(&release(&bueno, "sin hash")).is_none(), "sin hash");
        assert!(parse(&release(&bueno, "SHA256: 1234")).is_none(), "hash corto");
        assert!(parse(&release(&bueno, &format!("SHA256: {}", "z".repeat(64)))).is_none(), "hash no hexadecimal");
        assert!(parse(&json!({ "tag_name": "v2.1.0", "body": body, "assets": [] })).is_none(), "sin archivo");
        assert!(parse(&release("http://127.0.0.1:8765/BotoxOptimizer.exe", &body)).is_some(), "servidor local de pruebas");
    }

    #[test]
    fn la_api_solo_acepta_servidores_locales() {
        assert!(local("http://127.0.0.1:8765/r.json") && local("http://localhost/x"));
        assert!(!local("http://evil.com/r.json") && !local("https://127.0.0.1/x") && !local("http://127.0.0.1.evil.com/x") && !local("http://localhost.evil.com"));
    }
}
