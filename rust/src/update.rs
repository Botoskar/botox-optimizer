//! Actualización desde GitHub Releases.
//! Las versiones se publican como Releases del repositorio público del código (solo el .exe adjunto), así la app
//! no necesita ninguna clave. Antes de instalar se verifica el SHA256 en el mismo proceso que después ejecuta el
//! archivo, y en release se exige una firma Authenticode válida.
use crate::engine::{run_ps_with, Log};
use crate::sha256;
use crate::win;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REPO: &str = "Botoskar/botox-optimizer";
const ASSET: &str = "BotoxOptimizer.exe";
/// En release no se instala nada sin firma. Las pruebas locales (debug) sí pueden probar el flujo sin firmar.
const REQUIRE_SIGNATURE: bool = !cfg!(debug_assertions);
/// Publisher pinning: thumbprints SHA-1 de los certificados con los que se firman las versiones. Una firma válida
/// de cualquier otro certificado se rechaza. Es una lista para poder renovar el certificado: la versión firmada con
/// el viejo trae ya el nuevo. Vacía = en release no se instala ninguna actualización.
/// El certificado es autofirmado ("CN=Botox Optimizer", vence 2046-11-11, lo crea tools/new-signing-cert.ps1): Windows no
/// confía en su raíz, así que lo único que lo autentica es este thumbprint. Ver README > Sacar una versión nueva.
/// No hay variable de entorno para saltearlo: sería un bypass.
const PUBLISHER_THUMBPRINTS: &[&str] = &["2DBA1019FCB52F90A5692BA2F4FACF02976DABD7"];

/// true si el thumbprint (con o sin espacios, en cualquier caso) está en la lista fijada.
fn pin_matches(thumb: &str, pins: &[&str]) -> bool {
    let norm = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    let t = norm(thumb);
    t.len() == 40 && t.bytes().all(|c| c.is_ascii_hexdigit()) && pins.iter().any(|p| norm(p) == t)
}
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
    // Se escribe una copia de los bytes ya verificados y todo lo demás (firma, prueba, rename) se hace sobre esa copia.
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
    // La firma se mira en el archivo que se va a instalar, no en la descarga (antes eran dos archivos distintos).
    if REQUIRE_SIGNATURE {
        match win::authenticode_signer(&verified) {
            Err(e) => return discard(format!("El archivo no tiene una firma Authenticode válida ({e}). No se instaló nada.")),
            Ok(t) if !pin_matches(&t, PUBLISHER_THUMBPRINTS) => {
                return discard(format!("El archivo está firmado por otro certificado ({t}), no por el de Botox Optimizer. No se instaló nada."));
            }
            Ok(_) => {}
        }
    }
    // Antes de tocar el programa actual: la versión nueva tiene que arrancar y decir qué versión es.
    if let Err(e) = self_test(&verified, &u.version) {
        return discard(format!("La versión nueva no pasó la prueba de arranque ({e}). No se instaló nada."));
    }
    // Riesgo residual (TOCTOU): entre esta comprobación y el rename, alguien con escritura en la carpeta del .exe podría
    // cambiar el archivo; pero ese alguien ya puede reemplazar el .exe directamente. Se vuelve a calcular el hash igual.
    if !std::fs::read(&verified).is_ok_and(|b| sha256::hex(&b).eq_ignore_ascii_case(&u.sha256)) {
        return discard("La versión nueva cambió después de verificarla. No se instaló nada.".into());
    }
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
    // La versión nueva confirma al dibujar su primer cuadro; si no llega a hacerlo, en el arranque siguiente vuelve a la vieja.
    let _ = std::fs::write(pending_path(&exe), "0");
    Ok(())
}

/// Corre `exe --version` (sin ventana, máximo 20 s) y exige que conteste la versión esperada.
fn self_test(exe: &Path, expected: &str) -> Result<(), String> {
    use std::io::Read;
    let mut child = win::secure_command(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("no arrancó: {e}"))?;
    let t0 = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                return Err("no respondió".into());
            }
        }
    }
    let mut out = String::new();
    let _ = child.stdout.take().map(|mut s| s.read_to_string(&mut out));
    match (parse_ver(&out), parse_ver(expected)) {
        (Some(a), Some(b)) if a == b => Ok(()),
        _ => Err(format!("respondió {:?} en vez de {expected}", out.trim())),
    }
}

// ── Vuelta atrás si la versión nueva no arranca ──
// install deja "<exe>.update-pending" con "0". Al arrancar, la versión nueva lo pasa a "1"; al dibujar el primer cuadro
// lo borra junto con la copia vieja. Si arranca otra vez y encuentra "1", la vez anterior no llegó a dibujar: vuelve a la vieja.

fn pending_path(exe: &Path) -> PathBuf { exe.with_extension("update-pending") }

#[derive(Debug, PartialEq)]
enum Boot {
    /// Sin actualización a medio confirmar.
    Normal,
    /// Primer arranque de la versión nueva.
    FirstTry,
    /// La versión nueva ya arrancó una vez sin llegar a dibujar: volver a la anterior.
    RollBack,
}

fn boot_decision(marker: Option<&str>, old_exists: bool) -> Boot {
    match marker.map(str::trim) {
        None => Boot::Normal,
        // Sin la copia vieja no hay adónde volver.
        Some(_) if !old_exists => Boot::Normal,
        Some("0") => Boot::FirstTry,
        Some(_) => Boot::RollBack,
    }
}

/// Primera cosa que hace la app al abrir. Limpia lo que deja una actualización y, si la versión nueva no llegó a
/// arrancar bien la vez anterior, restaura la vieja y la abre (no vuelve).
pub fn on_start() {
    let Ok(exe) = std::env::current_exe() else { return };
    let (pending, old) = (pending_path(&exe), side(&exe, "old"));
    for tag in ["nuevo", "verified", "failed"] {
        let _ = std::fs::remove_file(side(&exe, tag));
    }
    match boot_decision(std::fs::read_to_string(&pending).ok().as_deref(), old.is_file()) {
        Boot::Normal => {
            let _ = std::fs::remove_file(&pending);
            let _ = std::fs::remove_file(&old);
        }
        Boot::FirstTry => {
            let _ = std::fs::write(&pending, "1");
        }
        Boot::RollBack => {
            let failed = side(&exe, "failed");
            if std::fs::rename(&exe, &failed).is_ok() {
                if std::fs::rename(&old, &exe).is_ok() {
                    let _ = std::fs::remove_file(&pending);
                    if win::secure_command(&exe).spawn().is_ok() {
                        std::process::exit(0);
                    }
                    return;
                }
                let _ = std::fs::rename(&failed, &exe);
            }
            // No se pudo volver atrás: se sigue con esta versión y se suelta el marcador para no intentarlo en cada arranque.
            let _ = std::fs::remove_file(&pending);
        }
    }
}

/// La versión nueva dibujó su primer cuadro: la actualización quedó bien y la copia vieja ya no hace falta.
pub fn confirm_ok() {
    let Ok(exe) = std::env::current_exe() else { return };
    if std::fs::remove_file(pending_path(&exe)).is_ok() {
        let _ = std::fs::remove_file(side(&exe, "old"));
    }
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

    #[test]
    fn el_pin_acepta_solo_los_certificados_fijados() {
        const A: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
        let pins = [A, "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"];
        assert!(pin_matches(A, &pins));
        assert!(pin_matches(&A.to_ascii_lowercase(), &pins), "sin importar mayúsculas");
        assert!(pin_matches("01 23 45 67 89 AB CD EF 01 23 45 67 89 AB CD EF 01 23 45 67", &pins), "con espacios, como lo copia certmgr");
        assert!(!pin_matches("1123456789ABCDEF0123456789ABCDEF01234567", &pins), "otro certificado");
        assert!(!pin_matches("", &[""]), "vacío no vale");
        assert!(!pin_matches(A, &[]), "sin certificados fijados no se acepta nada");
        assert!(!pin_matches(&A[..39], &[&A[..39]]), "largo incorrecto");
        assert!(PUBLISHER_THUMBPRINTS.iter().all(|p| p.len() == 40), "los fijados son thumbprints SHA-1 completos");
    }

    #[test]
    fn decide_cuando_volver_a_la_version_anterior() {
        assert_eq!(boot_decision(None, true), Boot::Normal, "sin marcador: arranque normal (y se borra la copia vieja)");
        assert_eq!(boot_decision(Some("0"), true), Boot::FirstTry, "primer arranque de la versión nueva");
        assert_eq!(boot_decision(Some("0\r\n"), true), Boot::FirstTry);
        assert_eq!(boot_decision(Some("1"), true), Boot::RollBack, "arrancó una vez y no llegó a dibujar");
        assert_eq!(boot_decision(Some("basura"), true), Boot::RollBack, "un marcador raro también vuelve atrás");
        assert_eq!(boot_decision(Some("1"), false), Boot::Normal, "sin copia vieja no hay adónde volver");
        assert_eq!(pending_path(Path::new(r"C:\x\BotoxOptimizer.exe")), PathBuf::from(r"C:\x\BotoxOptimizer.update-pending"));
        assert_eq!(side(Path::new(r"C:\x\BotoxOptimizer.exe"), "old"), PathBuf::from(r"C:\x\BotoxOptimizer.old.exe"));
    }

    #[test]
    fn la_prueba_de_arranque_exige_la_version_correcta() {
        // cmd.exe no entiende --version: no contesta una versión, así que no pasa.
        assert!(self_test(&win::system32_exe("cmd.exe"), "2.4.0").is_err());
        assert!(self_test(Path::new(r"C:\no\existe.exe"), "2.4.0").is_err());
    }
}
