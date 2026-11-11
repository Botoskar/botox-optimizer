//! Catálogo de tweaks y boosts. Se carga de assets/catalog.json (generado por tools/export-catalog.js).
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Clone)]
pub struct RegEntry {
    /// Ruta estilo PowerShell: `HKLM:\SYSTEM\...` o `HKCU:\...`
    pub p: String,
    pub n: String,
    pub v: Value,
    /// DWord | String | Binary
    pub t: String,
    /// Valor por defecto de Windows. null = el valor no existe por defecto.
    pub def: Value,
    /// Es el arranque de un servicio: solo se toca si el servicio existe.
    pub svc: bool,
}

#[derive(Deserialize, Clone)]
pub struct PwrEntry { pub sub: String, pub set: String, pub v: i64, pub def: i64 }

#[derive(Deserialize, Clone)]
pub struct Ps { pub apply: String, pub revert: String, pub check: String }

#[derive(Deserialize, Clone)]
pub struct Tweak {
    pub id: String,
    pub cat: String,
    pub name: String,
    pub desc: String,
    pub risk: String,
    pub reboot: bool,
    pub debloat: bool,
    pub uses_reg: bool,
    pub rec: Option<String>,
    pub needs: Option<String>,
    pub why: Option<String>,
    /// true: registro/servicios/energía que Rust maneja directo. false: usa los scripts de `ps`.
    pub native: bool,
    /// Los ajustes de energía se aplican solo enchufada (notebooks).
    pub ac: bool,
    pub reg: Vec<RegEntry>,
    pub pwr: Vec<PwrEntry>,
    pub ps: Option<Ps>,
}

#[derive(Deserialize, Clone)]
pub struct Boost { pub id: String, pub name: String, pub icon: String, pub every: String, pub desc: String, pub ps: String }

#[derive(Deserialize)]
pub struct Catalog {
    pub prelude: String,
    /// Script de PowerShell del escáner de hardware (solo lectura, devuelve JSON).
    pub scan: String,
    pub cats: serde_json::Map<String, Value>,
    pub tweaks: Vec<Tweak>,
    pub boosts: Vec<Boost>,
}

impl Catalog {
    pub fn load() -> Self { serde_json::from_str(include_str!("../assets/catalog.json")).expect("catalog.json inválido") }
    pub fn tweak(&self, id: &str) -> Option<&Tweak> { self.tweaks.iter().find(|t| t.id == id) }
    pub fn cat_name<'a>(&'a self, cat: &'a str) -> &'a str { self.cats.get(cat).and_then(|v| v.as_str()).unwrap_or(cat) }
}

/// Datos de la PC que usan las recomendaciones (rec) y la compatibilidad (needs).
#[derive(Clone, Default, Debug)]
pub struct Facts {
    pub ram_gb: u32,
    pub laptop: bool,
    pub c_free_gb: u32,
    pub gpu_vendor: String,
    pub sys_disk_ssd: bool,
    pub reserved_ok: bool,
    /// Radeon RX 6000 o más nueva (RDNA 2+): ahí HAGS se recomienda.
    pub amd_rdna2: bool,
    /// Ryzen X3D de doble chiplet (7900X3D, 7950X3D, 9900X3D, 9950X3D): su driver depende del estacionamiento de núcleos.
    pub x3d_dual: bool,
    /// Tweaks que en esta PC no tienen nada que cambiar (lo dice el chequeo, no el hardware).
    pub na: std::collections::HashSet<String>,
}

/// Evalúa una condición del catálogo. Son las funciones JS del catálogo original, portadas una por una.
pub fn cond(src: &str, f: &Facts) -> bool {
    let (nv, amd) = (f.gpu_vendor == "NVIDIA", f.gpu_vendor == "AMD");
    match src {
        "() => true" => true,
        "s => !s.laptop" => !f.laptop,
        "s => !s.laptop && !s.x3dDual" => !f.laptop && !f.x3d_dual,
        "s => s.laptop" => f.laptop,
        "s => s.gpuVendor === 'NVIDIA'" => nv,
        "s => s.gpuVendor === 'NVIDIA' || s.amdRdna2" => nv || f.amd_rdna2,
        "s => s.gpuVendor === 'AMD'" => amd,
        "s => s.gpuVendor === 'NVIDIA' || s.gpuVendor === 'AMD'" => nv || amd,
        "s => !!s.gpuVendor" => !f.gpu_vendor.is_empty(),
        "s => s.ramGB >= 8" => f.ram_gb >= 8,
        "s => s.ramGB >= 16" => f.ram_gb >= 16,
        "s => s.ramGB >= 32" => f.ram_gb >= 32,
        "s => s.sysDiskSSD" => f.sys_disk_ssd,
        "s => s.reservedOK" => f.reserved_ok,
        "s => s.reservedOK && s.cFreeGB < 40" => f.reserved_ok && f.c_free_gb < 40,
        other => panic!("condición del catálogo sin portar a Rust: {other}"),
    }
}

pub fn why(src: &str, f: &Facts) -> String {
    if src.contains("libera ~") {
        format!("Con {} GB de RAM libera ~{} GB en C:", f.ram_gb, (f.ram_gb as f32 * 0.4).round() as u32)
    } else if src.contains("Tenés") {
        format!("Tenés {} GB de RAM", f.ram_gb)
    } else if src.contains("tiene solo") {
        format!("C: tiene solo {} GB libres", f.c_free_gb)
    } else {
        panic!("motivo del catálogo sin portar a Rust: {src}")
    }
}

impl Tweak {
    /// Qué toca en Windows, en palabras, sacado del propio catálogo (#9). Los que usan PowerShell muestran su script.
    pub fn changes(&self) -> Vec<String> {
        let val = |v: &Value| match v {
            Value::Null => "no existe".to_string(),
            Value::String(s) => format!("\"{s}\""),
            Value::Array(a) => format!("binario de {} bytes", a.len()),
            v => v.to_string(),
        };
        let start = |v: &Value| match v.as_i64() { Some(2) => "Automático", Some(3) => "Manual", Some(4) => "Deshabilitado", _ => "otro" };
        let mut out: Vec<String> = self.reg.iter().map(|e| if e.svc {
            format!("Servicio {}: arranque {} (Windows: {})", e.p.rsplit('\\').next().unwrap_or(&e.p), start(&e.v), start(&e.def))
        } else {
            format!("{}\\{} = {} (Windows: {})", e.p.replacen(':', "", 1), e.n, val(&e.v), val(&e.def))
        }).collect();
        out.extend(self.pwr.iter().map(|x| format!("Plan de energía{}, ajuste {}: {} (Windows: {})", if self.ac { " (enchufada)" } else { "" }, x.set, x.v, x.def)));
        if self.ps.is_some() {
            out.push("Corre un script de PowerShell: es el que se ve abajo.".into());
        }
        out
    }
    pub fn applies(&self, f: &Facts) -> bool { !f.na.contains(&self.id) && self.needs.as_deref().map_or(true, |s| cond(s, f)) }
    pub fn recommended(&self, f: &Facts) -> bool { self.applies(f) && self.rec.as_deref().map_or(false, |s| cond(s, f)) }
    pub fn reason(&self, f: &Facts) -> String { self.why.as_deref().map(|s| why(s, f)).unwrap_or_default() }
}

/// "Optimizar para juegos": las recomendadas para esta PC que se **sabe** que están apagadas (con el estado sin leer,
/// aplicarlas de nuevo guardaría como respaldo los valores ya cambiados). Sin debloat ni las de notebook en una PC de
/// escritorio. Las de riesgo medio, solo si se piden.
pub fn one_click_ids(cat: &Catalog, f: &Facts, state: &std::collections::HashMap<String, bool>, medium: bool) -> Vec<String> {
    cat.tweaks
        .iter()
        .filter(|t| (t.cat != "laptop" || f.laptop) && !t.debloat && t.recommended(f) && state.get(&t.id) == Some(&false) && (t.risk == "low" || medium))
        .map(|t| t.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_clic_elige_recomendadas_apagadas_de_bajo_riesgo() {
        let c = Catalog::load();
        let f = Facts { ram_gb: 32, gpu_vendor: "NVIDIA".into(), sys_disk_ssd: true, ..Default::default() };
        let all_off: std::collections::HashMap<String, bool> = c.tweaks.iter().map(|t| (t.id.clone(), false)).collect();
        let low = one_click_ids(&c, &f, &all_off, false);
        assert!(!low.is_empty());
        for id in &low {
            let t = c.tweak(id).unwrap();
            assert!(t.risk == "low" && t.recommended(&f) && !t.debloat && t.cat != "laptop", "{id}");
        }
        // Toda recomendada de bajo riesgo que corresponde está incluida.
        let expected = c.tweaks.iter().filter(|t| t.risk == "low" && t.recommended(&f) && !t.debloat && t.cat != "laptop").count();
        assert_eq!(low.len(), expected);
        // Con riesgo medio se suman, sin perder las de bajo.
        let with_medium = one_click_ids(&c, &f, &all_off, true);
        assert!(with_medium.len() >= low.len() && low.iter().all(|id| with_medium.contains(id)));
        // Estado desconocido o ya activo: no se toca.
        assert!(one_click_ids(&c, &f, &Default::default(), true).is_empty());
        let mut one_on = all_off.clone();
        one_on.insert(low[0].clone(), true);
        assert!(!one_click_ids(&c, &f, &one_on, false).contains(&low[0]));
        // En una notebook sí entran las de su categoría.
        let lap = Facts { laptop: true, ..f.clone() };
        let lap_ids = one_click_ids(&c, &lap, &all_off, false);
        assert!(c.tweaks.iter().filter(|t| t.cat == "laptop" && t.risk == "low" && t.recommended(&lap)).all(|t| lap_ids.contains(&t.id)));
        // Lo que el chequeo marcó como "no aplica en esta PC" no se ofrece aunque figure apagado.
        let na = Facts { na: [low[0].clone()].into(), ..f.clone() };
        assert!(!one_click_ids(&c, &na, &all_off, true).contains(&low[0]));
        assert!(!c.tweak(&low[0]).unwrap().applies(&na));
    }

    #[test]
    fn que_cambia_se_lee_del_catalogo() {
        let c = Catalog::load();
        for t in &c.tweaks {
            assert!(!t.changes().is_empty(), "{} no dice qué cambia", t.id);
        }
        let reg = |p: &str, n: &str, v: Value, def: Value, svc: bool| RegEntry { p: p.into(), n: n.into(), v, t: "DWord".into(), def, svc };
        let mut t = c.tweaks.iter().find(|t| t.native).unwrap().clone();
        t.pwr.clear();
        t.reg = vec![reg("HKLM:\\SOFTWARE\\X", "Val", 1.into(), Value::Null, false), reg("HKLM:\\SYSTEM\\CurrentControlSet\\Services\\DiagTrack", "Start", 4.into(), 2.into(), true)];
        assert_eq!(t.changes(), vec!["HKLM\\SOFTWARE\\X\\Val = 1 (Windows: no existe)", "Servicio DiagTrack: arranque Deshabilitado (Windows: Automático)"]);
    }

    #[test]
    fn catalogo_completo_y_portado() {
        let c = Catalog::load();
        assert!(c.tweaks.len() >= 93);
        let f = Facts { ram_gb: 64, c_free_gb: 30, gpu_vendor: "NVIDIA".into(), ..Default::default() };
        for t in &c.tweaks {
            // No debe entrar en pánico: toda condición y motivo tiene su versión en Rust.
            let _ = (t.applies(&f), t.recommended(&f), t.reason(&f));
            assert!(c.cats.contains_key(&t.cat), "categoría desconocida en {}", t.id);
            if t.native {
                assert!(!t.reg.is_empty() || !t.pwr.is_empty(), "{} es nativo pero no tiene nada que aplicar", t.id);
                // El respaldo usa un solo archivo por tweak: no se mezclan registro y energía.
                assert!(t.reg.is_empty() || t.pwr.is_empty(), "{} mezcla registro y energía", t.id);
            } else {
                assert!(t.ps.is_some(), "{} no es nativo y no tiene scripts", t.id);
            }
        }
        let ids: std::collections::HashSet<_> = c.tweaks.iter().map(|t| &t.id).collect();
        assert_eq!(ids.len(), c.tweaks.len(), "hay ids repetidos");
    }

    fn con(f: Facts) -> impl Fn(&str) -> (bool, bool) {
        let c = Catalog::load();
        move |id| { let t = c.tweak(id).unwrap(); (t.applies(&f), t.recommended(&f)) }
    }

    #[test]
    fn ryzen_x3d_de_doble_chiplet_no_ofrece_lo_que_rompe_su_driver() {
        let normal = con(Facts { gpu_vendor: "NVIDIA".into(), ..Default::default() });
        assert_eq!((normal("coreparking"), normal("ultimate")), ((true, true), (true, true)));
        let x3d = con(Facts { x3d_dual: true, ..Default::default() });
        assert_eq!((x3d("coreparking"), x3d("ultimate")), ((false, false), (false, false)));
        // En una notebook tampoco, como antes.
        assert_eq!(con(Facts { laptop: true, ..Default::default() })("coreparking"), (false, false));
    }

    #[test]
    fn hags_se_recomienda_en_nvidia_y_en_radeon_rx_6000_o_mas() {
        let nv = con(Facts { gpu_vendor: "NVIDIA".into(), ..Default::default() });
        let rdna2 = con(Facts { gpu_vendor: "AMD".into(), amd_rdna2: true, ..Default::default() });
        let viejo = con(Facts { gpu_vendor: "AMD".into(), ..Default::default() }); // p. ej. una RX 580
        let intel = con(Facts { gpu_vendor: "Intel".into(), ..Default::default() });
        assert_eq!((nv("hags"), rdna2("hags"), viejo("hags"), intel("hags")), ((true, true), (true, true), (true, false), (false, false)));
    }

    #[test]
    fn la_telemetria_de_amd_toca_solo_los_servicios_de_telemetria() {
        // Este tweak lleva pasos propios de PowerShell: los servicios van dentro de los scripts generados (como en nvtelemetry).
        let c = Catalog::load();
        let ps = c.tweak("amdtelemetry").unwrap().ps.as_ref().expect("amdtelemetry usa PowerShell");
        for script in [&ps.apply, &ps.revert, &ps.check] {
            assert!(script.contains("Services\\AUEPLauncher") && script.contains("Services\\AMD Crash Defender Service"), "falta un servicio en: {script}");
            // Sin AMD External Events Utility FreeSync deja de andar: nunca se toca.
            assert!(!script.contains("External Events"), "no hay que tocar AMD External Events Utility");
        }
        assert!(ps.apply.contains("StopSvc 'AUEPLauncher'") && ps.apply.contains("StopSvc 'AMD Crash Defender Service'"));
    }
}
