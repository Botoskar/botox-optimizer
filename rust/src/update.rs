//! Actualización desde GitHub Releases.
//! Las versiones se publican en un repositorio público que solo trae el .exe, así la app
//! no necesita ninguna clave. Antes de instalar se verifica el SHA256 en el mismo proceso que después ejecuta el
//! archivo, y en release se exige una firma Authenticode válida.
use crate::engine::{run_ps_with, Log};
use crate::sha256;
use crate::win;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REPO: &str = "Botoskar/botox-optimizer-releases";
const ASSET: &str = "BotoxOptimizer.exe";
/// En release no se instala nada sin firma. Las pruebas locales (debug) sí pueden probar el flujo sin firmar.
const REQUIRE_SIGNATURE: bool = !cfg!(debug_assertions);
const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, PartialEq)]
struct UrlParts {
    scheme: String,
    host: String,
    port: Option<u16>,
}

/// Parser mínimo de URL: esquema, host y puerto. Rechaza credenciales (`usuario@host`), espacios y controles.
fn parse_url(url: &str) -> Option<UrlParts> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.is_empty() || authority.contains('@') || authority.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    if authority.contains(['[', ']']) {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => (h, Some(p.parse::<u16>().ok()?)),
        _ => (authority, None),
    };
    if host.is_empty() || host.contains('@') {
        return None;
    }
    Some(UrlParts { scheme, host: host.to_ascii_lowercase(), port })
}

/// Solo para pruebas: BX_UPDATE_API puede apuntar a un servidor de esta misma PC. En release siempre es false.
fn local(url: &str) -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    parse_url(url).is_some_and(|u| u.scheme == "http" && (u.host == "127.0.0.1" || u.host == "localhost"))
}

fn api_url() -> String {
    if cfg!(debug_assertions) {
        if let Ok(u) = std::env::var("BX_UPDATE_API") {
            if local(&u) {
                return u;
            }
        }
    }
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// La descarga solo puede venir del repo de versiones por HTTPS (o de localhost en builds de prueba).
fn trusted_download(url: &str) -> bool {
    if url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) {
        if let Some(u) = parse_url(url) {
            return u.scheme == "https" && u.host == "github.com" && u.port.is_none();
        }
    }
    local(url)
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
    // Solo se baja del repo de versiones (o de un servidor local en las pruebas), y nunca con credenciales en la URL.
    if !trusted_download(&url) {
        return None;
    }
    // El hash va en la descripción: "SHA256: <64 caracteres hexadecimales>".
    let body = json["body"].as_str()?;
    let sha256 = body.split_whitespace().skip_while(|w| !w.eq_ignore_ascii_case("sha256:")).nth(1)?.trim_matches('`').to_ascii_lowercase();
    if !(sha256.len() == 64 && sha256.bytes().all(|c| c.is_ascii_hexdigit())) {
        return None;
    }
    // GitHub publica además un digest por asset. Si está, tiene que coincidir con el de la descripción.
    if let Some(digest) = asset["digest"].as_str() {
        let digest = digest.trim().strip_prefix("sha256:").unwrap_or(digest).to_ascii_lowercase();
        if digest != sha256 {
            return None;
        }
    }
    Some(Update { version, url, sha256 })
}

/// Pregunta a GitHub cuál es la última versión. Ok(None) = ya estás al día.
pub fn check() -> Result<Option<Update>, String> {
    // La URL viaja por variable de entorno: nunca se interpola dentro del script de PowerShell.
    let url = api_url();
    let ps = "$ProgressPreference = 'SilentlyContinue'\n[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12\nInvoke-RestMethod -UseBasicParsing -TimeoutSec 30 -Headers @{ 'User-Agent' = 'BotoxOptimizer' } -Uri $env:BX_UPDATE_URL | ConvertTo-Json -Depth 6 -Compress";
    let o = run_ps_with("", ps, &Log(None), Duration::from_secs(45), &[("BX_UPDATE_URL", url.as_str())]);
    if !o.ok {
        return Err(o.err);
    }
    let json: Value = serde_json::from_str(o.out.trim()).map_err(|e| e.to_string())?;
    Ok(parse(&json).filter(|u| is_newer(&u.version, env!("CARGO_PKG_VERSION"))))
}

fn side(exe: &Path, tag: &str) -> PathBuf { exe.with_extension(format!("{tag}.exe")) }

/// Borra las copias que deja una actualización (no se puede mientras el programa viejo sigue abierto).
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        for tag in ["old", "nuevo", "verified"] {
            let _ = std::fs::remove_file(side(&exe, tag));
        }
    }
}

/// Baja el .exe, comprueba hash y firma, y lo pone en lugar del actual. El actual queda como "….old.exe" por si algo falla.
/// Windows deja renombrar un programa mientras corre, por eso no hace falta cerrarlo antes.
pub fn install(u: &Update) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (new, verified, old) = (side(&exe, "nuevo"), side(&exe, "verified"), side(&exe, "old"));
    let ps = "$ProgressPreference = 'SilentlyContinue'\n[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12\nInvoke-WebRequest -UseBasicParsing -TimeoutSec 180 -Headers @{ 'User-Agent' = 'BotoxOptimizer' } -Uri $env:BX_DL_URL -OutFile $env:BX_DL_OUT";
    let out_path = new.to_string_lossy().into_owned();
    let o = run_ps_with("", ps, &Log(None), Duration::from_secs(300), &[("BX_DL_URL", u.url.as_str()), ("BX_DL_OUT", out_path.as_str())]);
    let discard = |why: String| {
        let _ = std::fs::remove_file(&new);
        let _ = std::fs::remove_file(&verified);
        Err(why)
    };
    if !o.ok {
        return discard(format!("No se pudo descargar: {}", o.err));
    }
    // Se lee y verifica el archivo descargado en este proceso: ya no se confía en el hash que devuelve PowerShell.
    let size = std::fs::metadata(&new).map(|m| m.len()).unwrap_or(0);
    if size > MAX_DOWNLOAD {
        return discard("El archivo descargado es demasiado grande. No se instaló nada.".into());
    }
    let bytes = match std::fs::read(&new) {
        Ok(b) => b,
        Err(e) => return discard(format!("No se pudo leer el archivo descargado: {e}")),
    };
    if bytes.len() < 1_000_000 || !bytes.starts_with(b"MZ") {
        return discard("El archivo descargado no es un programa válido. No se instaló nada.".into());
    }
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return discard("El archivo descargado es demasiado grande. No se instaló nada.".into());
    }
    if !sha256::hex(&bytes).eq_ignore_ascii_case(&u.sha256) {
        return discard("El archivo descargado no coincide con el hash publicado. No se instaló nada.".into());
    }
    if REQUIRE_SIGNATURE {
        if let Err(e) = win::verify_authenticode(&new) {
            return discard(format!("El archivo no tiene una firma Authenticode válida ({e}). No se instaló nada."));
        }
    }
    // Se escribe una copia de los bytes ya verificados y se renombra esa copia: nadie puede cambiarla entre el hash y el rename.
    let _ = std::fs::remove_file(&verified);
    let write = || -> Result<(), String> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&verified).map_err(|e| format!("No se pudo preparar la versión nueva: {e}"))?;
        f.write_all(&bytes).map_err(|e| format!("No se pudo escribir la versión nueva: {e}"))?;
        f.sync_all().map_err(|e| format!("No se pudo escribir la versión nueva: {e}"))
    };
    if let Err(e) = write() {
        return discard(e);
    }
    let _ = std::fs::remove_file(&new);
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&verified);
        return Err(format!("No se pudo reemplazar el programa: {e}"));
    }
    if let Err(e) = std::fs::rename(&verified, &exe) {
        let _ = std::fs::rename(&old, &exe); // vuelve atrás
        let _ = std::fs::remove_file(&verified);
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
        json!({ "tag_name": "v2.1.0", "body": body, "assets": [{ "name": "otro.zip", "browser_download_url": "https://x" }, { "name": ASSET, "browser_download_url": url, "digest": format!("sha256:{SHA}") }] })
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
    fn rechaza_digest_que_no_coincide_con_la_descripcion() {
        let url = format!("https://github.com/{REPO}/releases/download/v2.1.0/{ASSET}");
        let mut j = release(&url, &format!("SHA256: {SHA}"));
        j["assets"][1]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        assert!(parse(&j).is_none(), "el digest del asset manda");
    }

    #[test]
    fn una_url_con_credenciales_no_es_localhost() {
        assert!(!local("http://localhost:80@evil.com/x"), "userinfo disfrazado de localhost");
        assert!(!local("http://127.0.0.1:8080@evil.com/x"));
        assert!(!local("https://127.0.0.1/x"));
    }

    #[test]
    fn la_api_solo_acepta_servidores_locales() {
        assert!(local("http://127.0.0.1:8765/r.json") && local("http://localhost/x"));
        assert!(!local("http://evil.com/r.json") && !local("https://127.0.0.1/x") && !local("http://127.0.0.1.evil.com/x") && !local("http://localhost.evil.com"));
        assert!(!local("http://localhost:80@evil.com/x"));
    }
}
