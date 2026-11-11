//! Escáner de hardware nativo con WMI.
//! Devuelve el mismo JSON que scan.js (el script de PowerShell), pero en una fracción de segundo.
//! Solo lee: no cambia nada en la PC. Si WMI falla, la app cae al script de PowerShell.
use crate::{engine::is_admin, reg, win};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::c_void;
use winreg::{enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE}, RegKey};
use wmi::{Variant, WMIConnection};

const GPU_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
const GB: f64 = (1u64 << 30) as f64;

// ── Utilidades ──

/// Consulta WQL; devuelve una fila por objeto. Sin conexión o con error, una lista vacía (el campo queda en su valor por defecto).
fn rows(con: &Option<WMIConnection>, wql: &str) -> Vec<Value> {
    let Some(con) = con else { return vec![] };
    con.raw_query::<HashMap<String, Variant>>(wql).unwrap_or_default().into_iter().map(|r| serde_json::to_value(r).unwrap_or_default()).collect()
}

/// WMI entrega los enteros de 64 bits como texto: se aceptan los dos.
fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Redondeo como [math]::Round de PowerShell (el .5 va al par más cercano). Los enteros se devuelven sin decimales.
fn rnd(x: f64, digits: i32) -> Value {
    let m = 10f64.powi(digits);
    let r = (x * m).round_ties_even() / m;
    if r.fract() == 0.0 { json!(r as i64) } else { json!(r) }
}

/// WMI manda la letra de unidad como texto de un carácter ("C"), o "\0" / nada si no tiene.
fn letter(v: &Value) -> Option<String> {
    v.as_str()?.chars().next().filter(|&c| c != '\0').map(String::from)
}

fn media(v: &Value) -> String {
    match num(v) as u32 { 0 => "Unspecified".into(), 3 => "HDD".into(), 4 => "SSD".into(), 5 => "SCM".into(), n => n.to_string() }
}

fn bus(v: &Value) -> String {
    const NAMES: [&str; 20] = ["Unknown", "SCSI", "ATAPI", "ATA", "1394", "SSA", "Fibre Channel", "USB", "RAID", "iSCSI", "SAS", "SATA", "SD", "MMC", "MAX", "File Backed Virtual", "Storage Spaces", "NVMe", "SCM", "UFS"];
    NAMES.get(num(v) as usize).map_or_else(|| num(v).to_string(), |s| s.to_string())
}

/// Velocidad del enlace como la muestra Windows: "1 Gbps", "866.7 Mbps".
fn link_speed(bps: f64) -> String {
    let fmt = |x: f64| { let t = format!("{:.1}", x); t.trim_end_matches('0').trim_end_matches('.').to_string() };
    if bps >= 1e9 { format!("{} Gbps", fmt(bps / 1e9)) } else if bps >= 1e6 { format!("{} Mbps", fmt(bps / 1e6)) } else { format!("{} Kbps", fmt(bps / 1e3)) }
}

/// Fecha de WMI ("20261109191355.500000-180": hora local y minutos de diferencia con UTC) a UTC.
fn wmi_utc(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let local = chrono::NaiveDateTime::parse_from_str(s.get(..14)?, "%Y%m%d%H%M%S").ok()?;
    let offset: i64 = s.get(21..)?.parse().ok()?;
    Some((local - chrono::Duration::minutes(offset)).and_utc())
}

/// "20220427000000.000000+000" → "2022-04-27". La fecha de la BIOS no lleva hora: se toma tal cual.
fn wmi_date(s: &str) -> String {
    match (s.get(..4), s.get(4..6), s.get(6..8)) { (Some(y), Some(m), Some(d)) => format!("{y}-{m}-{d}"), _ => String::new() }
}

// ── Registro ──

/// VRAM real por nombre de driver (AdapterRAM de WMI se queda en 4 GB).
fn vram_by_name() -> HashMap<String, f64> {
    let mut out = HashMap::new();
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(GPU_CLASS) else { return out };
    for sub in class.enum_keys().flatten() {
        let path = format!(r"HKLM:\{GPU_CLASS}\{sub}");
        let (Some((Value::String(desc), _)), Some((mem, _))) = (reg::get(&path, "DriverDesc"), reg::get(&path, "HardwareInformation.qwMemorySize")) else { continue };
        // Según el driver viene como número o como 8 bytes.
        let bytes = match &mem {
            Value::Array(b) => b.iter().enumerate().map(|(i, x)| (x.as_u64().unwrap_or(0) as f64) * 256f64.powi(i as i32)).sum(),
            other => num(other),
        };
        if bytes > 0.0 {
            out.insert(desc.to_lowercase(), bytes);
        }
    }
    out
}

fn reg_num(path: &str, name: &str) -> Option<f64> { reg::get(path, name).map(|(v, _)| num(&v)) }

fn reg_text(path: &str, name: &str) -> String { reg::get(path, name).map(|(v, _)| text(&v)).unwrap_or_default() }

// ── Windows ──

#[repr(C)]
struct Guid { a: u32, b: u16, c: u16, d: [u8; 8] }

#[link(name = "powrprof")]
extern "system" {
    fn PowerGetActiveScheme(root: *mut c_void, scheme: *mut *mut Guid) -> u32;
    fn PowerReadFriendlyName(root: *mut c_void, scheme: *const Guid, sub: *const Guid, setting: *const Guid, buf: *mut u8, size: *mut u32) -> u32;
}
#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(p: *mut c_void) -> *mut c_void;
}
#[link(name = "shlwapi")]
extern "system" {
    fn SHLoadIndirectString(src: *const u16, out: *mut u16, cch: u32, reserved: *mut c_void) -> i32;
}

fn from_utf16(buf: &[u16]) -> String { String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]) }

/// Los planes que trae Windows guardan el nombre como recurso ("@%SystemRoot%\system32\powrprof.dll,-14"): se resuelve al idioma actual.
fn resolve_indirect(s: &str) -> String {
    if !s.starts_with('@') {
        return s.to_string();
    }
    let src: Vec<u16> = s.encode_utf16().chain([0]).collect();
    let mut out = [0u16; 256];
    let hr = unsafe { SHLoadIndirectString(src.as_ptr(), out.as_mut_ptr(), out.len() as u32, std::ptr::null_mut()) };
    if hr == 0 { from_utf16(&out) } else { s.to_string() }
}

/// (nombre, GUID en minúsculas) del plan de energía activo. No necesita administrador ni lanzar powercfg.
fn power_plan() -> (String, String) {
    unsafe {
        let mut g: *mut Guid = std::ptr::null_mut();
        if PowerGetActiveScheme(std::ptr::null_mut(), &mut g) != 0 || g.is_null() {
            return Default::default();
        }
        let id = &*g;
        let guid = format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}", id.a, id.b, id.c, id.d[0], id.d[1], id.d[2], id.d[3], id.d[4], id.d[5], id.d[6], id.d[7]);
        let mut buf = [0u16; 512];
        let mut size = (buf.len() * 2) as u32;
        let ok = PowerReadFriendlyName(std::ptr::null_mut(), g, std::ptr::null(), std::ptr::null(), buf.as_mut_ptr() as *mut u8, &mut size) == 0;
        LocalFree(g as *mut c_void);
        (if ok { resolve_indirect(&from_utf16(&buf)) } else { String::new() }, guid)
    }
}

/// Versión del driver de NVIDIA como la informa nvidia-smi (el de WMI sale con otro formato).
fn nvidia_driver() -> Option<String> {
    let o = win::secure_command(win::nvidia_smi()?).args(["--query-gpu=driver_version", "--format=csv,noheader"]).output().ok()?;
    let line = String::from_utf8_lossy(&o.stdout).lines().next()?.trim().to_string();
    (o.status.success() && !line.is_empty()).then_some(line)
}

// ── Escaneo ──

pub fn scan() -> Result<Value, String> {
    let cim = Some(WMIConnection::new().map_err(|e| format!("WMI no disponible: {e}"))?);
    let storage = WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\Storage").ok();
    let net = WMIConnection::with_namespace_path(r"ROOT\StandardCimv2").ok();
    let guard = WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\DeviceGuard").ok();
    let first = |v: Vec<Value>| v.into_iter().next().unwrap_or_default();

    let cpu = first(rows(&cim, "SELECT Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, L3CacheSize FROM Win32_Processor"));
    let cs = first(rows(&cim, "SELECT TotalPhysicalMemory, HypervisorPresent FROM Win32_ComputerSystem"));
    // VBS: el estado 2 solo dice que el hipervisor corre (Hyper-V, WSL2 y Docker lo mantienen); lo que importa es qué funciones lo usan.
    let dg = first(rows(&guard, "SELECT VirtualizationBasedSecurityStatus, SecurityServicesRunning FROM Win32_DeviceGuard"));
    let os = first(rows(&cim, "SELECT Caption, BuildNumber, LastBootUpTime FROM Win32_OperatingSystem"));
    let board = first(rows(&cim, "SELECT Manufacturer, Product FROM Win32_BaseBoard"));
    let bios = first(rows(&cim, "SELECT SMBIOSBIOSVersion, ReleaseDate FROM Win32_BIOS"));
    let nt = r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion";

    // GPUs: VRAM real del registro y driver de NVIDIA de nvidia-smi.
    let vram = vram_by_name();
    let raw_gpus = rows(&cim, "SELECT Name, AdapterRAM, DriverVersion, CurrentRefreshRate, CurrentHorizontalResolution, CurrentVerticalResolution FROM Win32_VideoController");
    let nv = raw_gpus.iter().any(|g| text(&g["Name"]).to_lowercase().contains("nvidia")).then(nvidia_driver).flatten();
    let gpus: Vec<Value> = raw_gpus
        .iter()
        .map(|g| {
            let name = text(&g["Name"]);
            let bytes = vram.get(&name.to_lowercase()).copied().unwrap_or_else(|| num(&g["AdapterRAM"]));
            let driver = match &nv { Some(d) if name.to_lowercase().contains("nvidia") => d.clone(), _ => text(&g["DriverVersion"]) };
            json!({ "name": name, "vramGB": rnd(bytes / GB, 1), "driver": driver, "refresh": g["CurrentRefreshRate"],
                    "res": format!("{}x{}", text(&g["CurrentHorizontalResolution"]), text(&g["CurrentVerticalResolution"])) })
        })
        .collect();

    let modules: Vec<Value> = rows(&cim, "SELECT Capacity, Speed, ConfiguredClockSpeed, Manufacturer, PartNumber, DeviceLocator, SMBIOSMemoryType FROM Win32_PhysicalMemory")
        .iter()
        .map(|m| json!({ "gb": rnd(num(&m["Capacity"]) / GB, 0), "speed": m["Speed"], "conf": m["ConfiguredClockSpeed"], "maker": text(&m["Manufacturer"]), "part": text(&m["PartNumber"]), "slot": m["DeviceLocator"], "type": m["SMBIOSMemoryType"] }))
        .collect();

    // Discos: letras de unidad por número de disco.
    let letters: Vec<(String, String)> = rows(&storage, "SELECT DiskNumber, DriveLetter FROM MSFT_Partition").iter().filter_map(|p| Some((num(&p["DiskNumber"]).to_string(), letter(&p["DriveLetter"])?))).collect();
    let disks: Vec<Value> = rows(&storage, "SELECT DeviceId, FriendlyName, MediaType, BusType, Size FROM MSFT_PhysicalDisk")
        .iter()
        .map(|d| {
            let id = text(&d["DeviceId"]);
            json!({ "name": text(&d["FriendlyName"]), "media": media(&d["MediaType"]), "bus": bus(&d["BusType"]), "sizeGB": rnd(num(&d["Size"]) / GB, 0),
                    "letters": letters.iter().filter(|(n, _)| *n == id).map(|(_, l)| l.clone()).collect::<Vec<_>>() })
        })
        .collect();
    // Volúmenes fijos con letra. MSFT_Volume (lo que lee Get-Volume) deja afuera las unidades virtuales de subst.
    let mut vols: Vec<(String, Value)> = rows(&storage, "SELECT DriveLetter, DriveType, FileSystemLabel, Size, SizeRemaining FROM MSFT_Volume")
        .iter()
        .filter(|v| num(&v["DriveType"]) == 3.0)
        .filter_map(|v| Some((letter(&v["DriveLetter"])?, json!({ "label": v["FileSystemLabel"], "sizeGB": rnd(num(&v["Size"]) / GB, 0), "freeGB": rnd(num(&v["SizeRemaining"]) / GB, 0) }))))
        .collect();
    vols.sort_by(|a, b| a.0.cmp(&b.0));
    let vols: Vec<Value> = vols.into_iter().map(|(l, mut v)| { v["letter"] = json!(l); v }).collect();

    // Red: solo adaptadores físicos conectados.
    let dns = rows(&net, "SELECT InterfaceIndex, ServerAddresses FROM MSFT_DNSClientServerAddress WHERE AddressFamily = 2");
    let mut adapters = rows(&net, "SELECT Name, InterfaceDescription, InterfaceIndex, HardwareInterface, Hidden, NdisPhysicalMedium, InterfaceOperationalStatus, MediaConnectState, Speed FROM MSFT_NetAdapter");
    adapters.sort_by_key(|a| num(&a["InterfaceIndex"]) as u32);
    let nics: Vec<Value> = adapters
        .iter()
        .filter(|a| a["HardwareInterface"] == true && a["Hidden"] != true && num(&a["InterfaceOperationalStatus"]) == 1.0 && num(&a["MediaConnectState"]) == 1.0)
        .map(|a| {
            let servers: Vec<String> = dns.iter().filter(|d| num(&d["InterfaceIndex"]) == num(&a["InterfaceIndex"])).flat_map(|d| d["ServerAddresses"].as_array().cloned().unwrap_or_default()).filter_map(|s| s.as_str().map(String::from)).collect();
            json!({ "name": a["Name"], "desc": a["InterfaceDescription"], "speed": link_speed(num(&a["Speed"])), "wifi": num(&a["NdisPhysicalMedium"]) == 9.0, "dns": servers.join(", ") })
        })
        .collect();

    let (plan, plan_guid) = power_plan();
    let sys_drive = std::env::var("SystemDrive").unwrap_or_default().trim_end_matches(':').to_string();
    let uptime = os["LastBootUpTime"].as_str().and_then(wmi_utc).map_or(0.0, |t| (chrono::Utc::now() - t).num_seconds() as f64 / 3600.0);
    let reserved = match reg_num(r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\ReserveManager", "ShippedWithReserves") { Some(v) if v == 0.0 => "Disabled", Some(_) => "Enabled", None => "" };
    let join = |a: &Value, b: &Value| format!("{} {}", text(a), text(b)).trim().to_string();

    let mut spec = json!({
        "pc": std::env::var("COMPUTERNAME").unwrap_or_default(),
        "admin": is_admin(),
        "laptop": !rows(&cim, "SELECT Name FROM Win32_Battery").is_empty(),
        "cpu": { "name": text(&cpu["Name"]), "maker": cpu["Manufacturer"], "cores": cpu["NumberOfCores"], "threads": cpu["NumberOfLogicalProcessors"], "mhz": cpu["MaxClockSpeed"], "l3MB": rnd(num(&cpu["L3CacheSize"]) / 1024.0, 0) },
        "gpus": gpus,
        "ram": { "gb": rnd(num(&cs["TotalPhysicalMemory"]) / GB, 0), "modules": modules },
        "disks": disks,
        "vols": vols,
        "sysDrive": sys_drive,
        "nics": nics,
        "board": join(&board["Manufacturer"], &board["Product"]),
        "bios": format!("{} ({})", text(&bios["SMBIOSBIOSVersion"]), wmi_date(bios["ReleaseDate"].as_str().unwrap_or_default())),
        "os": { "name": os["Caption"], "ver": reg_text(nt, "DisplayVersion"), "build": format!("{}.{}", text(&os["BuildNumber"]), reg_num(nt, "UBR").unwrap_or(0.0)), "uptimeH": rnd(uptime, 1) },
        "vbs": dg["VirtualizationBasedSecurityStatus"].as_f64().unwrap_or(0.0) as i64,
        "vbsRunning": dg["SecurityServicesRunning"].as_array().cloned().unwrap_or_default(),
        "hypervisor": cs["HypervisorPresent"] == true,
        "hags": reg_num(r"HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode"),
        "plan": plan,
        "reserved": reserved,
        "planGuid": plan_guid,
    });
    if text(&cpu["Name"]).to_uppercase().contains("X3D") {
        spec["x3d"] = x3d_info();
    }
    Ok(spec)
}

/// Lo que necesita un Ryzen X3D de doble chiplet para que el driver de V-Cache mande los juegos al chiplet con caché:
/// el servicio del driver de chipset de AMD, el Modo Juego de Windows y Xbox Game Bar. Solo se lee.
fn x3d_info() -> Value {
    let packages = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
    let game_bar = RegKey::predef(HKEY_CURRENT_USER).open_subkey(packages).is_ok_and(|k| k.enum_keys().flatten().any(|n| n.starts_with("Microsoft.XboxGamingOverlay_")));
    json!({
        "svc": reg::key_exists(r"HKLM:\SYSTEM\CurrentControlSet\Services\amd3dvcacheSvc"),
        // Sin valor, Windows lo deja encendido.
        "gameMode": reg_num(r"HKCU:\Software\Microsoft\GameBar", "AutoGameModeEnabled") != Some(0.0),
        "gameBar": game_bar,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lee las tres comprobaciones en esta PC (solo lectura). Esta PC no es X3D, pero Modo Juego y Game Bar se pueden leer igual.
    #[test]
    fn datos_de_vcache_se_leen_sin_fallar() {
        let x = x3d_info();
        assert!(x["svc"].is_boolean() && x["gameMode"].is_boolean() && x["gameBar"].is_boolean(), "{x}");
    }

    #[test]
    fn letras_de_unidad() {
        assert_eq!((letter(&json!("C")), letter(&json!("\u{0}")), letter(&Value::Null), letter(&json!(""))), (Some("C".into()), None, None, None));
    }

    #[test]
    fn redondea_como_powershell() {
        assert_eq!((rnd(2.5, 0), rnd(3.5, 0), rnd(7.45, 1), rnd(32768.0 / 1024.0, 0)), (json!(2), json!(4), json!(7.4), json!(32)));
        assert_eq!(rnd(8.0, 1), json!(8));
    }

    #[test]
    fn velocidades_de_enlace() {
        assert_eq!((link_speed(1e9), link_speed(2.5e9), link_speed(100e6), link_speed(866.7e6), link_speed(10e9)), ("1 Gbps".into(), "2.5 Gbps".into(), "100 Mbps".into(), "866.7 Mbps".into(), "10 Gbps".into()));
    }

    #[test]
    fn fechas_de_wmi() {
        // Argentina: UTC-3 (-180 min). 19:13:55 locales = 22:13:55 UTC.
        let t = wmi_utc("20261109191355.500000-180").unwrap();
        assert_eq!(t.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-11-09 22:13:55");
        assert_eq!(wmi_utc("20260101000000.000000+060").unwrap().format("%H").to_string(), "23");
        assert!(wmi_utc("basura").is_none());
        assert_eq!(wmi_date("20220427000000.000000+000"), "2022-04-27");
        assert_eq!(wmi_date(""), "");
    }

    #[test]
    fn nombres_de_disco() {
        assert_eq!((media(&json!(4)), media(&json!(0)), media(&json!(3)), bus(&json!(17)), bus(&json!(11)), bus(&json!(99))), ("SSD".into(), "Unspecified".into(), "HDD".into(), "NVMe".into(), "SATA".into(), "99".into()));
        assert_eq!(num(&json!("17179869184")), 17179869184.0); // WMI manda los uint64 como texto
    }

    #[test]
    fn los_planes_de_windows_traen_el_nombre_como_recurso() {
        let name = resolve_indirect(r"@%SystemRoot%\system32\powrprof.dll,-14");
        assert!(!name.is_empty() && !name.starts_with('@'), "no se resolvió: {name}");
        assert_eq!(resolve_indirect("Mi plan"), "Mi plan");
    }

    /// Lee el hardware real de esta PC (solo lectura, sin administrador).
    #[test]
    fn escanea_esta_pc() {
        let spec = scan().expect("WMI");
        assert!(!text(&spec["cpu"]["name"]).is_empty() && num(&spec["cpu"]["threads"]) >= 1.0, "{}", spec["cpu"]);
        assert!(num(&spec["ram"]["gb"]) >= 1.0 && !spec["ram"]["modules"].as_array().unwrap().is_empty());
        assert!(!spec["gpus"].as_array().unwrap().is_empty() && !spec["disks"].as_array().unwrap().is_empty());
        assert!(!spec["vols"].as_array().unwrap().is_empty() && text(&spec["sysDrive"]).len() == 1);
        assert!(text(&spec["os"]["build"]).contains('.') && !text(&spec["os"]["name"]).is_empty());
        let guid = text(&spec["planGuid"]);
        assert!(guid.len() == 36 && guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{guid}");
        assert!(!text(&spec["plan"]).is_empty());
        assert!(spec["vbsRunning"].is_array() && spec["hypervisor"].is_boolean(), "VBS: {} {}", spec["vbsRunning"], spec["hypervisor"]);
        // Lo que consume la app: los hechos y los hallazgos se arman sin fallar con el escaneo nativo.
        let f = crate::sys::facts(&spec);
        assert!(f.ram_gb >= 1 && !crate::sys::findings(&spec, &f).is_empty());
    }
}
