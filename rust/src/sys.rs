//! Datos de la PC: escáner de hardware, hallazgos y uso en vivo de CPU / RAM / GPU.
use crate::catalog::{Catalog, Facts};
use crate::engine::{run_ps, Log};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::process::Command;

/// Escaneo de hardware: nativo con WMI; si WMI falla, cae al script de PowerShell (más lento, mismo resultado).
pub fn scan(cat: &Catalog) -> Result<Value, String> {
    crate::hwscan::scan().or_else(|_| scan_ps(cat))
}

pub fn scan_ps(cat: &Catalog) -> Result<Value, String> {
    let o = run_ps("", &cat.scan, &Log(None));
    let json = o.out.find('{').map(|i| &o.out[i..]).unwrap_or_default();
    serde_json::from_str(json).map_err(|_| if o.err.is_empty() { "el escáner no devolvió datos".to_string() } else { o.err })
}

fn n(v: &Value) -> f64 { v.as_f64().unwrap_or(0.0) }
fn s(v: &Value) -> &str { v.as_str().unwrap_or_default() }

/// La GPU principal: la de más VRAM (deja afuera adaptadores virtuales).
pub fn main_gpu(spec: &Value) -> &Value {
    spec["gpus"].as_array().and_then(|g| g.iter().max_by(|a, b| n(&a["vramGB"]).total_cmp(&n(&b["vramGB"])))).unwrap_or(&Value::Null)
}

pub fn facts(spec: &Value) -> Facts {
    let sys = s(&spec["sysDrive"]);
    let gpu = s(&main_gpu(spec)["name"]).to_lowercase();
    let sys_disk = spec["disks"].as_array().and_then(|d| d.iter().find(|d| d["letters"].as_array().is_some_and(|l| l.iter().any(|x| s(x) == sys))));
    let c_free = spec["vols"].as_array().and_then(|v| v.iter().find(|v| s(&v["letter"]) == sys)).map_or(999.0, |v| n(&v["freeGB"]));
    Facts {
        ram_gb: n(&spec["ram"]["gb"]) as u32,
        laptop: spec["laptop"].as_bool().unwrap_or(false) || std::env::var("BX_LAPTOP").as_deref() == Ok("1"), // BX_LAPTOP=1: simula notebook
        c_free_gb: c_free as u32,
        gpu_vendor: if gpu.contains("nvidia") { "NVIDIA" } else if gpu.contains("amd") || gpu.contains("radeon") { "AMD" } else if gpu.contains("intel") { "Intel" } else { "" }.into(),
        sys_disk_ssd: sys_disk.is_some_and(|d| s(&d["media"]).to_lowercase().contains("ssd") || s(&d["bus"]).to_lowercase().contains("nvme")),
        reserved_ok: !s(&spec["reserved"]).is_empty(),
        amd_rdna2: is_rdna2(&gpu),
        x3d_dual: is_x3d_dual(s(&spec["cpu"]["name"]), n(&spec["cpu"]["cores"])),
    }
}

/// Radeon RX 6000 o más nueva: "RX " y 4 cifras que empiezan en 6-9. La RX 580 (3 cifras) y la RX 5700 XT quedan afuera.
fn is_rdna2(gpu_lower: &str) -> bool {
    gpu_lower.contains("radeon") && gpu_lower.split("rx ").skip(1).any(|r| {
        let d: String = r.chars().take_while(char::is_ascii_digit).collect();
        d.len() == 4 && matches!(d.as_bytes()[0], b'6'..=b'9')
    })
}

/// X3D con dos chiplets (7900X3D, 7950X3D, 9900X3D, 9950X3D): 12 núcleos o más. Los de un solo chiplet (5800X3D, 7800X3D, 9800X3D) tienen 8.
fn is_x3d_dual(cpu: &str, cores: f64) -> bool { cpu.to_uppercase().contains("X3D") && cores >= 12.0 }

/// Ryzen de escritorio basados en chips de APU: solo manejan PCIe 3.0, aunque la placa madre soporte 4.0.
fn pcie3_only(cpu: &str) -> bool {
    let c = format!("{} ", cpu.to_uppercase());
    ["5700 ", "5500 ", "5600G", "5700G", "5600GT", "5500GT", "5300G", "4500 ", "4100 ", "4600G", "4700G"].iter().any(|m| c.contains(&format!("RYZEN 3 {m}")) || c.contains(&format!("RYZEN 5 {m}")) || c.contains(&format!("RYZEN 7 {m}")))
}

/// Hallazgos del escáner: (nivel ok|warn|bad, título, explicación).
pub fn findings(spec: &Value, f: &Facts) -> Vec<(&'static str, String, String)> {
    let mut out = vec![];
    let mut add = |lvl: &'static str, t: String, d: &str| out.push((lvl, t, d.to_string()));
    let gpu = main_gpu(spec);
    let mods = spec["ram"]["modules"].as_array().cloned().unwrap_or_default();
    let speed = mods.iter().map(|m| if n(&m["conf"]) > 0.0 { n(&m["conf"]) } else { n(&m["speed"]) }).fold(f64::MAX, f64::min);
    let ddr = match mods.first().map(|m| n(&m["type"]) as u32) { Some(34) => "DDR5", Some(26) => "DDR4", _ => "" };
    if !mods.is_empty() {
        if (ddr == "DDR4" && speed < 2900.0) || (ddr == "DDR5" && speed < 5000.0) {
            add("warn", format!("RAM a {speed:.0} MHz"), "Parece que XMP/DOCP/EXPO está apagado. Activalo en la BIOS para usar la velocidad real de tu memoria (+5-15% de FPS cuando limita el CPU).");
        } else {
            add("ok", format!("RAM a {speed:.0} MHz {ddr}"), "La memoria corre a buena velocidad.");
        }
        if mods.len() >= 2 {
            add("ok", format!("{} módulos de RAM", mods.len()), "Funcionando en dual channel (si están en las ranuras correctas).");
        } else {
            add("warn", "Un solo módulo de RAM".into(), "Single channel reduce mucho los FPS en CPU. Sumá un segundo módulo igual.");
        }
    }
    let hz = n(&gpu["refresh"]);
    if hz > 0.0 {
        if hz <= 60.0 {
            add("bad", format!("Pantalla a {hz:.0} Hz"), "Si tu monitor soporta más Hz, cambialo en Configuración › Pantalla › Pantalla avanzada.");
        } else {
            add("ok", format!("Pantalla a {hz:.0} Hz ({})", s(&gpu["res"])), "Frecuencia de actualización alta activa.");
        }
    }
    let cpu = s(&spec["cpu"]["name"]);
    if pcie3_only(cpu) {
        add("warn", "El CPU limita la placa de video a PCIe 3.0".into(), "Este Ryzen solo maneja PCIe 3.0. Las placas de 8 líneas (RTX 4060, RX 7600) pierden ancho de banda: se nota cuando se llena la VRAM. Bajá texturas si ves tirones.");
    }
    let (l3, cores) = (n(&spec["cpu"]["l3MB"]), n(&spec["cpu"]["cores"]));
    if l3 > 0.0 && l3 <= 16.0 && cores >= 6.0 {
        add("warn", format!("Poca caché L3 ({l3:.0} MB)"), "Los juegos competitivos dependen mucho de la caché del CPU. Es normal ver menos FPS y más caídas que en un CPU con 32 MB.");
    }
    if f.c_free_gb < 30 {
        add("warn", format!("C: con {} GB libres", f.c_free_gb), "Poco espacio en el disco del sistema. Probá Boost-Up y \"Desactivar hibernación\".");
    }
    if n(&spec["vbs"]) == 2.0 {
        add("warn", "VBS (seguridad basada en virtualización) activa".into(), "Puede costar 5-10% de FPS. Ver \"Desactivar VBS\" en Optimizaciones.");
    } else {
        add("ok", "VBS desactivada".into(), "Sin costo de rendimiento por virtualización.");
    }
    if (f.gpu_vendor == "NVIDIA" || f.amd_rdna2) && n(&spec["hags"]) != 2.0 {
        add("warn", "HAGS desactivado".into(), "La programación de GPU acelerada baja la latencia de entrada en tu placa de video. Activala en Optimizaciones.");
    }
    if f.x3d_dual {
        // Solo se afirma lo que el escáner pudo leer; con el escáner de respaldo (sin estos datos) no se dice nada.
        let x = &spec["x3d"];
        if x["svc"].as_bool() == Some(false) {
            add("bad", "Falta el driver de V-Cache de AMD".into(), "Tu Ryzen X3D tiene dos chiplets y solo uno lleva la caché extra. Sin el driver de chipset de AMD, los juegos pueden caer en el otro y rendir menos. Instalá el último desde amd.com.");
        } else if x["gameMode"].as_bool() == Some(false) {
            add("warn", "Modo Juego apagado".into(), "El driver de V-Cache de AMD usa el Modo Juego de Windows para reconocer los juegos. Activalo en Configuración › Juegos.");
        } else if x["gameBar"].as_bool() == Some(false) {
            add("warn", "Falta Xbox Game Bar".into(), "El driver de V-Cache de AMD la necesita para reconocer los juegos. Instalala desde Microsoft Store (no hace falta usarla).");
        } else if x["svc"].as_bool() == Some(true) {
            add("ok", "V-Cache de AMD listo".into(), "Driver de chipset, Modo Juego y Game Bar en orden: los juegos van al chiplet con caché.");
        }
    }
    let plan = s(&spec["planGuid"]).to_lowercase();
    if plan.starts_with("381b4222") || plan.starts_with("a1841308") {
        add("warn", format!("Plan de energía: {}", s(&spec["plan"])), "Usá \"Máximo rendimiento\" en Optimizaciones › Energía.");
    }
    let up = n(&spec["os"]["uptimeH"]);
    if up > 72.0 {
        add("warn", format!("Sin reiniciar hace {:.0} días", up / 24.0), "Reiniciar libera memoria y aplica cambios pendientes.");
    }
    for nic in spec["nics"].as_array().into_iter().flatten() {
        let speed = s(&nic["speed"]);
        if nic["wifi"].as_bool() == Some(true) {
            add("warn", format!("Conectado por Wi-Fi ({})", s(&nic["name"])), "Para jugar online, el cable reduce el ping y el jitter.");
        } else if speed.to_lowercase().contains("mbps") && speed.split_whitespace().next().and_then(|x| x.parse::<f64>().ok()).is_some_and(|x| x < 1000.0) {
            add("warn", format!("Red a {speed}"), "El enlace no llega a 1 Gbps: revisá el cable (Cat5e o mejor).");
        }
    }
    if f.laptop {
        add("warn", "Notebook detectada".into(), "Las optimizaciones de energía que gastan batería no aplican. Probá la pestaña Notebook (rendimiento máximo solo con cargador) y \"Usar la GPU dedicada en los juegos\" en la pestaña Juegos.");
    }
    out
}

// ── Uso en vivo ──

#[repr(C)]
#[derive(Default)]
struct FileTime { lo: u32, hi: u32 }
#[repr(C)]
struct MemoryStatusEx { len: u32, load: u32, total: u64, avail: u64, _rest: [u64; 5] }

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
    fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
}

/// Tiempos acumulados del CPU: (inactivo, total). El uso sale de la diferencia entre dos lecturas.
pub fn cpu_times() -> (u64, u64) {
    let (mut i, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default());
    unsafe { GetSystemTimes(&mut i, &mut k, &mut u) };
    let v = |f: &FileTime| ((f.hi as u64) << 32) | f.lo as u64;
    (v(&i), v(&k) + v(&u)) // el tiempo de kernel ya incluye el inactivo
}

#[derive(Clone, Default, Debug)]
pub struct Stats {
    pub cpu: Option<u32>,
    pub ram: u32,
    pub ram_used_gb: f32,
    /// (uso %, VRAM %, temperatura °C). Solo NVIDIA, vía nvidia-smi.
    pub gpu: Option<(u32, u32, u32)>,
}

pub fn stats(prev: (u64, u64)) -> (Stats, (u64, u64)) {
    let now = cpu_times();
    let (idle, total) = (now.0.saturating_sub(prev.0), now.1.saturating_sub(prev.1));
    let cpu = (prev.1 > 0 && total > 0).then(|| (100.0 * (1.0 - idle as f64 / total as f64)).round().clamp(0.0, 100.0) as u32);
    let mut m = MemoryStatusEx { len: std::mem::size_of::<MemoryStatusEx>() as u32, load: 0, total: 0, avail: 0, _rest: [0; 5] };
    unsafe { GlobalMemoryStatusEx(&mut m) };
    let gpu = Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu", "--format=csv,noheader,nounits"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            let v: Vec<f32> = text.lines().next()?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 4 && v[2] > 0.0).then(|| (v[0] as u32, (100.0 * v[1] / v[2]).round() as u32, v[3] as u32))
        });
    (Stats { cpu, ram: m.load, ram_used_gb: (m.total - m.avail) as f32 / (1u64 << 30) as f32, gpu }, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detecta_cpu_pcie3() {
        assert!(pcie3_only("AMD Ryzen 7 5700 8-Core Processor"));
        assert!(pcie3_only("AMD Ryzen 5 5600G with Radeon Graphics"));
        assert!(!pcie3_only("AMD Ryzen 7 5700X 8-Core Processor"));
        assert!(!pcie3_only("AMD Ryzen 5 5600 6-Core Processor"));
        assert!(!pcie3_only("AMD Ryzen 7 5700X3D 8-Core Processor"));
    }

    #[test]
    fn datos_y_hallazgos_de_una_pc_de_ejemplo() {
        // La PC del amigo: Ryzen 7 5700 + RX 7600.
        let spec = json!({
            "sysDrive": "C", "laptop": false, "reserved": "", "vbs": 0, "hags": 2, "planGuid": "381b4222-f694-41f0-9685-ff5bb260df2e", "plan": "Equilibrado",
            "cpu": { "name": "AMD Ryzen 7 5700 8-Core Processor", "cores": 8, "l3MB": 16 },
            "gpus": [{ "name": "Virtual", "vramGB": 0 }, { "name": "AMD Radeon RX 7600", "vramGB": 8, "refresh": 240, "res": "1920x1080" }],
            "ram": { "gb": 16, "modules": [{ "conf": 3000, "type": 26 }, { "conf": 3000, "type": 26 }] },
            "disks": [{ "media": "SSD", "bus": "NVMe", "letters": ["C"] }], "vols": [{ "letter": "C", "freeGB": 465 }],
            "os": { "uptimeH": 0.7 }, "nics": [{ "name": "Ethernet", "speed": "100 Mbps", "wifi": false }]
        });
        let f = facts(&spec);
        assert_eq!((f.ram_gb, f.c_free_gb, f.gpu_vendor.as_str(), f.sys_disk_ssd, f.reserved_ok, f.laptop), (16, 465, "AMD", true, false, false));
        assert!(f.amd_rdna2 && !f.x3d_dual, "RX 7600 sí es RDNA 2+; un Ryzen 7 5700 no es X3D");
        let titles: Vec<String> = findings(&spec, &f).into_iter().map(|x| x.1).collect();
        for expected in ["RAM a 3000 MHz DDR4", "PCIe 3.0", "Poca caché L3 (16 MB)", "Plan de energía: Equilibrado", "Red a 100 Mbps", "Pantalla a 240 Hz"] {
            assert!(titles.iter().any(|t| t.contains(expected)), "falta el hallazgo '{expected}' en {titles:?}");
        }
    }

    #[test]
    fn detecta_radeon_rx_6000_o_mas() {
        for (name, esperado) in [
            ("amd radeon rx 7600", true), ("amd radeon rx 6600 xt", true), ("amd radeon rx 9070 xt", true), ("amd radeon rx 7900 xtx", true),
            ("amd radeon rx 580 series", false), ("amd radeon rx 5700 xt", false), ("amd radeon rx vega 56", false),
            ("amd radeon(tm) graphics", false), ("nvidia geforce rtx 4060", false), ("amd radeon 780m graphics", false),
        ] {
            assert_eq!(is_rdna2(name), esperado, "{name}");
        }
    }

    #[test]
    fn detecta_ryzen_x3d_de_doble_chiplet() {
        for (cpu, nucleos, esperado) in [
            ("AMD Ryzen 9 7950X3D 16-Core Processor", 16.0, true), ("AMD Ryzen 9 7900X3D 12-Core Processor", 12.0, true), ("AMD Ryzen 9 9950X3D 16-Core Processor", 16.0, true),
            ("AMD Ryzen 7 7800X3D 8-Core Processor", 8.0, false), ("AMD Ryzen 7 5800X3D 8-Core Processor", 8.0, false), ("AMD Ryzen 9 7950X 16-Core Processor", 16.0, false),
        ] {
            assert_eq!(is_x3d_dual(cpu, nucleos), esperado, "{cpu}");
        }
    }

    #[test]
    fn hallazgos_de_vcache_solo_con_datos_del_escaneo() {
        let base = |x3d: serde_json::Value| {
            let mut spec = json!({
                "sysDrive": "C", "laptop": false, "reserved": "", "vbs": 0, "hags": 2, "planGuid": "381b4222-f694-41f0-9685-ff5bb260df2e", "plan": "Equilibrado",
                "cpu": { "name": "AMD Ryzen 9 7950X3D 16-Core Processor", "cores": 16, "l3MB": 128 }, "gpus": [{ "name": "NVIDIA GeForce RTX 4080", "vramGB": 16 }],
                "ram": { "gb": 32, "modules": [] }, "disks": [], "vols": [{ "letter": "C", "freeGB": 400 }], "os": { "uptimeH": 1 }, "nics": []
            });
            if !x3d.is_null() { spec["x3d"] = x3d; }
            let f = facts(&spec);
            assert!(f.x3d_dual);
            findings(&spec, &f).into_iter().filter(|x| x.1.contains("V-Cache") || x.1.contains("Modo Juego") || x.1.contains("Game Bar")).map(|x| (x.0, x.1)).collect::<Vec<_>>()
        };
        assert_eq!(base(json!({ "svc": false, "gameMode": true, "gameBar": true })), [("bad", "Falta el driver de V-Cache de AMD".to_string())]);
        assert_eq!(base(json!({ "svc": true, "gameMode": false, "gameBar": true })), [("warn", "Modo Juego apagado".to_string())]);
        assert_eq!(base(json!({ "svc": true, "gameMode": true, "gameBar": false })), [("warn", "Falta Xbox Game Bar".to_string())]);
        assert_eq!(base(json!({ "svc": true, "gameMode": true, "gameBar": true })), [("ok", "V-Cache de AMD listo".to_string())]);
        assert!(base(serde_json::Value::Null).is_empty(), "sin datos del escaneo no se afirma nada");
    }

    #[test]
    fn uso_en_vivo() {
        let (_, t0) = stats((0, 0));
        std::thread::sleep(std::time::Duration::from_millis(120));
        let (s, _) = stats(t0);
        assert!(s.cpu.is_some_and(|c| c <= 100) && s.ram > 0 && s.ram <= 100 && s.ram_used_gb > 0.5, "{s:?}");
    }
}
