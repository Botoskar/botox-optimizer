//! Rutas y procesos de Windows que no dependen de variables de entorno.
//! Un proceso elevado no puede confiar en `PATH`, `TEMP`, `SystemRoot` ni en el directorio actual:
//! un proceso sin privilegios puede fijarlos antes de que se apruebe el UAC. Acá se resuelven con
//! APIs de Windows y se arma un entorno mínimo para los hijos (PowerShell, sc, powercfg, etc.).

use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const NO_WINDOW: u32 = 0x0800_0000;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}

const PROGRAM_DATA: Guid = Guid { a: 0x62AB_5D82, b: 0xFDC1, c: 0x4DC3, d: [0xA9, 0xDD, 0x07, 0x0D, 0x1D, 0x49, 0x5D, 0x97] };
const LOCAL_APP_DATA: Guid = Guid { a: 0xF1B3_270E, b: 0x7D55, c: 0x4D3E, d: [0x9D, 0x73, 0x4C, 0x0B, 0xA8, 0x0B, 0x2E, 0xE9] };
const ROAMING_APP_DATA: Guid = Guid { a: 0x3EB6_85DB, b: 0x65F9, c: 0x4CF6, d: [0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F, 0x3D] };
const USER_PROFILE: Guid = Guid { a: 0x5E6C_858F, b: 0x0E22, c: 0x4760, d: [0x9A, 0xFE, 0xEA, 0x33, 0x17, 0xB6, 0x71, 0x73] };
const PROGRAM_FILES: Guid = Guid { a: 0x905E_63B6, b: 0xC1BF, c: 0x494E, d: [0xB2, 0x9C, 0x65, 0xB7, 0x32, 0xD3, 0xD2, 0x1A] };
const PROGRAM_FILES_X86: Guid = Guid { a: 0x7C5A_40EF, b: 0xA0FB, c: 0x4BFC, d: [0x87, 0x4A, 0xC0, 0xF2, 0xE0, 0xB9, 0xFA, 0x8E] };

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buf: *mut u16, size: u32) -> u32;
    fn GetWindowsDirectoryW(buf: *mut u16, size: u32) -> u32;
}

#[link(name = "shell32")]
extern "system" {
    fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: isize, path: *mut *mut u16) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoTaskMemFree(p: *mut c_void);
}

fn wide_buf(f: unsafe extern "system" fn(*mut u16, u32) -> u32) -> PathBuf {
    let mut buf = vec![0u16; 32_768];
    let n = unsafe { f(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return PathBuf::new();
    }
    PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}

pub fn windows_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let p = wide_buf(GetWindowsDirectoryW);
        if p.as_os_str().is_empty() { PathBuf::from(r"C:\Windows") } else { p }
    })
    .as_path()
}

pub fn system32() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let p = wide_buf(GetSystemDirectoryW);
        if p.as_os_str().is_empty() { windows_dir().join("System32") } else { p }
    })
    .as_path()
}

pub fn system32_exe(name: &str) -> PathBuf {
    system32().join(name)
}

pub fn powershell() -> PathBuf {
    system32().join(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// Carpeta conocida de Windows. Si la API falla se usa una ruta derivada del sistema, nunca una variable de entorno.
pub fn known_folder(id: &Guid) -> Option<PathBuf> {
    let mut p: *mut u16 = std::ptr::null_mut();
    let hr = unsafe { SHGetKnownFolderPath(id, 0, 0, &mut p) };
    if hr != 0 || p.is_null() {
        return None;
    }
    let mut len = 0usize;
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) });
    unsafe { CoTaskMemFree(p as *mut c_void) };
    (!s.is_empty()).then(|| PathBuf::from(s))
}

pub fn program_data() -> PathBuf {
    known_folder(&PROGRAM_DATA).unwrap_or_else(|| windows_dir().join("ProgramData"))
}

pub fn local_app_data() -> PathBuf {
    known_folder(&LOCAL_APP_DATA).unwrap_or_else(|| user_profile().join(r"AppData\Local"))
}

pub fn roaming_app_data() -> PathBuf {
    known_folder(&ROAMING_APP_DATA).unwrap_or_else(|| user_profile().join(r"AppData\Roaming"))
}

pub fn user_profile() -> PathBuf {
    known_folder(&USER_PROFILE).unwrap_or_else(|| windows_dir().parent().unwrap_or(Path::new(r"C:\")).join("Users"))
}

pub fn program_files() -> PathBuf {
    known_folder(&PROGRAM_FILES).unwrap_or_else(|| windows_dir().parent().unwrap_or(Path::new(r"C:\")).join("Program Files"))
}

pub fn program_files_x86() -> PathBuf {
    known_folder(&PROGRAM_FILES_X86).unwrap_or_else(program_files)
}

pub fn system_drive() -> String {
    match windows_dir().components().next() {
        Some(Component::Prefix(p)) => p.as_os_str().to_string_lossy().into_owned(),
        _ => "C:".into(),
    }
}

fn joined(paths: impl IntoIterator<Item = PathBuf>) -> OsString {
    std::env::join_paths(paths).unwrap_or_default()
}

/// Entorno de un hijo elevado: rutas de Windows reales y un PATH fijo. No borra todo el entorno
/// (PowerShell necesita USERNAME, COMPUTERNAME, etc.), pero pisa lo que decide qué se ejecuta y dónde se escribe.
pub fn secure_command(exe: impl AsRef<Path>) -> Command {
    let sys = system32();
    let win = windows_dir();
    let local = local_app_data();
    let roaming = roaming_app_data();
    let temp = local.join("Temp");
    let pf = program_files();
    let pf86 = program_files_x86();
    let pd = program_data();
    let path = joined(vec![
        sys.to_path_buf(),
        sys.join("Wbem"),
        sys.join(r"WindowsPowerShell\v1.0"),
        pf.join(r"NVIDIA Corporation\NVSMI"),
        // Sin %LOCALAPPDATA%\Microsoft\WindowsApps: es del usuario. winget se resuelve por ruta absoluta (WingetExe).
    ]);
    // Solo módulos del sistema: una ruta de módulos del perfil sería escribible por el usuario y
    // podría cargarse dentro de un proceso elevado (bypass de UAC).
    let modules = joined([
        sys.join(r"WindowsPowerShell\v1.0\Modules"),
        pf.join(r"WindowsPowerShell\Modules"),
        pf86.join(r"WindowsPowerShell\Modules"),
    ]);

    let mut c = Command::new(exe.as_ref().as_os_str());
    c.creation_flags(NO_WINDOW);
    c.current_dir(sys);
    c.env("SystemRoot", win.as_os_str());
    c.env("windir", win.as_os_str());
    c.env("SystemDrive", system_drive());
    c.env("ProgramData", pd.as_os_str());
    c.env("LOCALAPPDATA", local.as_os_str());
    c.env("APPDATA", roaming.as_os_str());
    c.env("USERPROFILE", user_profile().as_os_str());
    c.env("TEMP", temp.as_os_str());
    c.env("TMP", temp.as_os_str());
    c.env("ProgramFiles", pf.as_os_str());
    c.env("ProgramFiles(x86)", pf86.as_os_str());
    c.env("ProgramW6432", pf.as_os_str());
    c.env("PATH", path);
    c.env("PSModulePath", modules);
    // Los hooks de prueba no deben llegar a un hijo elevado.
    for k in ["BX_UPDATE_API", "BX_BACKUP_DIR", "BX_SHOT", "BX_DEMO", "BX_CONFIRM", "BX_PAGE", "BX_TAB", "BX_REBOOT", "BX_LAPTOP", "BX_FAKE"] {
        c.env_remove(k);
    }
    c
}

/// `nvidia-smi` sin buscarlo en PATH. Si no está en las ubicaciones normales, no hay datos de GPU.
pub fn nvidia_smi() -> Option<PathBuf> {
    let a = system32_exe("nvidia-smi.exe");
    if a.is_file() {
        return Some(a);
    }
    let b = program_files().join(r"NVIDIA Corporation\NVSMI\nvidia-smi.exe");
    b.is_file().then_some(b)
}

/// Un junction o symlink en ProgramData permitiría redirigir escrituras elevadas a otra carpeta.
pub fn is_reparse_point(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_attributes() & 0x0000_0400 != 0).unwrap_or(false)
}

#[link(name = "advapi32")]
extern "system" {
    fn GetNamedSecurityInfoW(name: *const u16, obj: u32, info: u32, owner: *mut *mut c_void, group: *mut *mut c_void, dacl: *mut *mut c_void, sacl: *mut *mut c_void, sd: *mut *mut c_void) -> u32;
    fn IsWellKnownSid(sid: *mut c_void, kind: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(p: *mut c_void) -> *mut c_void;
}

/// true si el dueño del archivo o carpeta es SYSTEM o Administradores.
/// `icacls /grant:r` no cambia el dueño, y el dueño siempre puede volver a darse permisos: una carpeta que creó
/// un usuario normal (ProgramData lo deja) no sirve para lo que después se ejecuta o se aplica elevado.
pub fn owner_is_admin(p: &Path) -> Result<bool, String> {
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    let (mut owner, mut sd) = (std::ptr::null_mut(), std::ptr::null_mut());
    // 1 = SE_FILE_OBJECT, 1 = OWNER_SECURITY_INFORMATION
    let err = unsafe { GetNamedSecurityInfoW(wide.as_ptr(), 1, 1, &mut owner, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd) };
    if err != 0 {
        return Err(format!("no se pudo leer el dueño de {} (error {err})", p.display()));
    }
    // 22 = WinLocalSystemSid, 26 = WinBuiltinAdministratorsSid
    let ok = !owner.is_null() && unsafe { IsWellKnownSid(owner, 22) != 0 || IsWellKnownSid(owner, 26) != 0 };
    unsafe { LocalFree(sd) };
    Ok(ok)
}

/// Exige que `p` sea de SYSTEM/Administradores y no sea un enlace. Para usar antes de escribir o aplicar algo elevado.
pub fn require_admin_owned(p: &Path) -> Result<(), String> {
    if is_reparse_point(p) {
        return Err(format!("{} es un enlace/reparse point: no se usa por seguridad", p.display()));
    }
    match owner_is_admin(p)? {
        true => Ok(()),
        false => Err(format!("{} no pertenece a SYSTEM ni a Administradores (lo creó otro usuario): no se usa por seguridad. Si la creaste vos, borrala desde una consola de administrador y volvé a intentar", p.display())),
    }
}

/// Deja una carpeta solo para SYSTEM y Administradores (sin herencia). La usan los respaldos.
/// Primero exige que la carpeta ya sea de un administrador: si no, los permisos se podrían volver a abrir.
pub fn harden_dir(dir: &Path) -> Result<(), String> {
    require_admin_owned(dir)?;
    // Sin /C: si falla en un archivo de adentro, icacls corta y devuelve error en vez de seguir.
    let out = secure_command(system32_exe("icacls.exe"))
        .arg(dir.as_os_str())
        .args(["/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F", "/T", "/Q"])
        .output()
        .map_err(|e| format!("no se pudo ejecutar icacls: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("no se pudieron endurecer los permisos de {}: {}", dir.display(), if msg.is_empty() { format!("icacls devolvió {}", out.status.code().unwrap_or(-1)) } else { msg }));
    }
    Ok(())
}

/// Carpeta donde se escriben los .ps1. Si la app está elevada va a ProgramData con ACL de administrador;
/// si no, al perfil del usuario (sin frontera de elevación que cruzar).
pub fn script_dir() -> Result<PathBuf, String> {
    static DIR: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let admin = crate::engine::is_admin();
        let root = if admin { program_data().join("BotoxOptimizer") } else { local_app_data().join("BotoxOptimizer") };
        let dir = root.join("tmp");
        std::fs::create_dir_all(&dir).map_err(|e| format!("no se pudo crear la carpeta temporal: {e}"))?;
        for p in [&root, &dir] {
            if is_reparse_point(p) {
                return Err(format!("{} es un enlace/reparse point: no se usa por seguridad", p.display()));
            }
        }
        if admin {
            // El dueño de la carpeta padre podría renombrar `tmp` y poner otra en su lugar: también tiene que ser de un admin.
            require_admin_owned(&root)?;
            harden_dir(&dir)?;
        }
        Ok(dir)
    })
    .clone()
}

// ── Firma Authenticode ──
// WinVerifyTrust valida la cadena y la firma incrustada; `authenticode_signer` además devuelve el thumbprint del
// firmante, que el actualizador compara con los fijados (publisher pinning).

#[repr(C)]
struct FileInfo {
    cb_struct: u32,
    file_path: *const u16,
    file_handle: isize,
    known_subject: *const Guid,
}

#[repr(C)]
struct TrustData {
    cb_struct: u32,
    policy_callback: *mut c_void,
    sip_client: *mut c_void,
    ui_choice: u32,
    revocation_checks: u32,
    union_choice: u32,
    file: *mut FileInfo,
    state_action: u32,
    state_data: isize,
    url_reference: *const u16,
    prov_flags: u32,
    ui_context: u32,
    signature_settings: *mut c_void,
}

const ACTION_GENERIC_VERIFY_V2: Guid = Guid { a: 0x00AA_C56B, b: 0xCD44, c: 0x11D0, d: [0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE] };

#[link(name = "wintrust")]
extern "system" {
    fn WinVerifyTrust(hwnd: isize, action: *const Guid, data: *mut c_void) -> i32;
    fn WTHelperProvDataFromStateData(state: isize) -> *mut c_void;
    fn WTHelperGetProvSignerFromChain(prov: *mut c_void, signer: u32, counter: i32, counter_idx: u32) -> *mut c_void;
    fn WTHelperGetProvCertFromChain(signer: *mut c_void, cert: u32) -> *mut ProviderCert;
}

#[link(name = "crypt32")]
extern "system" {
    fn CertGetCertificateContextProperty(cert: *const c_void, prop: u32, data: *mut u8, len: *mut u32) -> i32;
}

/// Comienzo de CRYPT_PROVIDER_CERT: solo hace falta el puntero al certificado.
#[repr(C)]
struct ProviderCert {
    cb_struct: u32,
    cert: *const c_void,
}

/// CERT_E_UNTRUSTEDROOT: la firma y el archivo están bien, pero la raíz no es una CA instalada (certificado autofirmado).
const CERT_E_UNTRUSTEDROOT: i32 = 0x800B_0109_u32 as i32;

/// Verifica la firma Authenticode y devuelve el thumbprint SHA-1 del certificado firmante, en hexadecimal mayúscula.
/// Acepta una raíz no confiable (certificado autofirmado): **el resultado solo vale comparado con un thumbprint fijado**
/// (`update.rs` lo exige siempre). Un archivo adulterado o sin firma falla antes, con otro código (p. ej. TRUST_E_BAD_DIGEST).
pub fn authenticode_signer(path: &Path) -> Result<String, String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut fi = FileInfo { cb_struct: std::mem::size_of::<FileInfo>() as u32, file_path: wide.as_ptr(), file_handle: 0, known_subject: std::ptr::null() };
    let mut td = TrustData {
        cb_struct: std::mem::size_of::<TrustData>() as u32,
        policy_callback: std::ptr::null_mut(),
        sip_client: std::ptr::null_mut(),
        ui_choice: 2,         // WTD_UI_NONE
        revocation_checks: 0, // WTD_REVOKE_NONE (sin red: no colgar la actualización)
        union_choice: 1,      // WTD_CHOICE_FILE
        file: &mut fi as *mut FileInfo,
        state_action: 1, // WTD_STATEACTION_VERIFY: guarda el estado para leer el firmante (se cierra abajo)
        state_data: 0,
        url_reference: std::ptr::null(),
        prov_flags: 0x0000_0010, // WTD_REVOCATION_CHECK_NONE
        ui_context: 0,
        signature_settings: std::ptr::null_mut(),
    };
    let hr = unsafe { WinVerifyTrust(0, &ACTION_GENERIC_VERIFY_V2, &mut td as *mut TrustData as *mut c_void) };
    let signed_ok = hr == 0 || hr == CERT_E_UNTRUSTEDROOT;
    let thumb = signed_ok.then(|| unsafe {
        let prov = WTHelperProvDataFromStateData(td.state_data);
        let signer = if prov.is_null() { std::ptr::null_mut() } else { WTHelperGetProvSignerFromChain(prov, 0, 0, 0) };
        let cert = if signer.is_null() { std::ptr::null_mut() } else { WTHelperGetProvCertFromChain(signer, 0) };
        if cert.is_null() || (*cert).cert.is_null() {
            return None;
        }
        let (mut buf, mut len) = ([0u8; 20], 20u32);
        // 3 = CERT_SHA1_HASH_PROP_ID (el "Thumbprint" que muestra Windows)
        (CertGetCertificateContextProperty((*cert).cert, 3, buf.as_mut_ptr(), &mut len) != 0 && len == 20).then(|| buf.iter().map(|b| format!("{b:02X}")).collect::<String>())
    });
    td.state_action = 2; // WTD_STATEACTION_CLOSE
    unsafe { WinVerifyTrust(0, &ACTION_GENERIC_VERIFY_V2, &mut td as *mut TrustData as *mut c_void) };
    match (signed_ok, thumb.flatten()) {
        (true, Some(t)) => Ok(t),
        (true, None) => Err("firma válida pero no se pudo leer el certificado firmante".into()),
        _ => Err(format!("firma Authenticode ausente o inválida (0x{:08X})", hr as u32)),
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attrs: *const c_void, owner: i32, name: *const u16) -> isize;
    fn GetLastError() -> u32;
    fn CloseHandle(h: isize) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

/// Instancia única: dos ventanas elevadas tocando registro, servicios y respaldos a la vez se pisan.
/// Mutex con nombre en `Local\` (por sesión; no un archivo en una carpeta escribible). Si ya hay otra, la trae al frente
/// (o avisa) y devuelve false. Reintenta unos segundos: al actualizar, la versión vieja abre la nueva y recién ahí se cierra.
pub fn single_instance() -> bool {
    let name = wide(r"Local\BotoxOptimizer.Instancia");
    for _ in 0..30 {
        let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        let err = unsafe { GetLastError() };
        // 183 = ERROR_ALREADY_EXISTS. 5 = ERROR_ACCESS_DENIED: la otra instancia está elevada y esta no.
        if h != 0 && err != 183 {
            return true; // el handle queda abierto hasta que se cierre el proceso
        }
        if h != 0 {
            unsafe { CloseHandle(h) };
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let hwnd = unsafe { FindWindowW(std::ptr::null(), wide("Botox Optimizer").as_ptr()) };
    if hwnd == 0 || unsafe { SetForegroundWindow(hwnd) } == 0 {
        unsafe { MessageBoxW(0, wide("Botox Optimizer ya está abierto.").as_ptr(), wide("Botox Optimizer").as_ptr(), 0x40) };
    } else {
        unsafe { ShowWindow(hwnd, 9) }; // SW_RESTORE, por si estaba minimizada
    }
    false
}

/// Variables solo para pruebas y capturas. En release no se leen: un proceso inferior no puede activar hooks.
#[cfg(debug_assertions)]
pub fn test_env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

#[cfg(not(debug_assertions))]
pub fn test_env(_k: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rutas_del_sistema() {
        assert!(system32().is_dir(), "{}", system32().display());
        assert!(windows_dir().is_dir(), "{}", windows_dir().display());
        assert!(powershell().is_file(), "{}", powershell().display());
        assert!(program_data().is_absolute());
        assert!(local_app_data().is_absolute());
        assert!(program_files().is_absolute());
    }

    #[test]
    fn los_hijos_no_heredan_el_dir_actual_ni_un_path_raro() {
        let c = secure_command(system32_exe("cmd.exe"));
        let envs: Vec<_> = c.get_envs().map(|(k, v)| (k.to_string_lossy().to_ascii_lowercase(), v.map(|x| x.to_string_lossy().into_owned()))).collect();
        assert!(envs.iter().any(|(k, _)| k.as_str() == "path"));
        assert!(envs.iter().any(|(k, _)| k.as_str() == "systemroot"));
        assert_eq!(c.get_current_dir(), Some(system32()));
        let path = envs.iter().find(|(k, _)| k.as_str() == "path").and_then(|(_, v)| v.clone()).unwrap();
        assert!(!path.contains("BX_"), "{path}");
    }

    #[test]
    fn una_carpeta_de_un_usuario_normal_no_sirve_para_lo_elevado() {
        // Lo que se crea acá es del usuario, como lo que plantaría un atacante en ProgramData. Elevado (los runners de
        // GitHub corren como admin) lo creado es de Administradores y la prueba no aplica.
        if crate::engine::is_admin() {
            eprintln!("corriendo elevado: se saltea");
            return;
        }
        let dir = std::env::temp_dir().join(format!("bx-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(owner_is_admin(&dir), Ok(false));
        assert!(require_admin_owned(&dir).is_err());
        assert!(harden_dir(&dir).is_err(), "sin dueño admin no se endurece (se podría volver a abrir)");
        let _ = std::fs::remove_dir_all(&dir);
        // ProgramData es de SYSTEM: el chequeo reconoce un dueño administrativo.
        assert_eq!(owner_is_admin(&program_data()), Ok(true), "{}", program_data().display());
        assert!(owner_is_admin(Path::new(r"C:\no\existe\bx")).is_err());
    }

    #[test]
    fn la_firma_devuelve_el_thumbprint_y_rechaza_lo_que_no_esta_firmado() {
        // Sin firma: el propio ejecutable de los tests.
        assert!(authenticode_signer(&std::env::current_exe().unwrap()).is_err());
        // Con firma incrustada (los de System32 suelen estar firmados por catálogo, que esto no acepta).
        let signed = [program_files().join(r"GitHub CLI\gh.exe"), program_files().join(r"Git\cmd\git.exe"), program_files().join(r"PowerShell\7\pwsh.exe")];
        let Some(exe) = signed.iter().find(|p| p.is_file()) else {
            eprintln!("no hay un .exe con firma incrustada para probar: se saltea");
            return;
        };
        let t = authenticode_signer(exe).unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
        assert_eq!(t.len(), 40, "{t}");
        assert!(t.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()), "{t}");
    }

    #[test]
    fn autofirmado_da_su_thumbprint_y_un_byte_cambiado_lo_invalida() {
        // Certificado autofirmado de prueba en CurrentUser\My: se crea, firma una copia de este .exe y se borra con su
        // clave en el mismo script (finally). No toca el almacén de raíces de confianza.
        let dir = std::env::temp_dir().join(format!("bx-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("firmado.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let script = format!(
            "$ErrorActionPreference='Stop'; $c = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=BotoxTestFirma' -CertStoreLocation Cert:\\CurrentUser\\My -NotAfter (Get-Date).AddDays(1); \
             try {{ $s = Set-AuthenticodeSignature -LiteralPath '{}' -Certificate $c -HashAlgorithm SHA256; if (-not $s.SignerCertificate) {{ throw $s.StatusMessage }}; $c.Thumbprint }} \
             finally {{ Remove-Item -LiteralPath ('Cert:\\CurrentUser\\My\\' + $c.Thumbprint) -DeleteKey }}",
            exe.display()
        );
        // secure_command fija el PSModulePath del sistema: heredado de pwsh 7 (runners de GitHub), PowerShell 5.1 no
        // carga el módulo que crea la unidad Cert:.
        let out = secure_command(system32().join(r"WindowsPowerShell\v1.0\powershell.exe")).args(["-NoProfile", "-NonInteractive", "-Command", &script]).output().unwrap();
        let thumb = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(out.status.success() && thumb.len() == 40, "no se pudo firmar: {thumb} {}", String::from_utf8_lossy(&out.stderr));
        // Raíz no confiable, pero la firma es íntegra: devuelve el firmante para compararlo con el pin.
        assert_eq!(authenticode_signer(&exe), Ok(thumb));
        // Un byte del código cambiado: el digest no coincide y se rechaza, aunque el certificado sea el mismo.
        let mut bytes = std::fs::read(&exe).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        let bad = dir.join("adulterado.exe");
        std::fs::write(&bad, bytes).unwrap();
        assert!(authenticode_signer(&bad).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
