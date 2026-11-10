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
}

/// Evalúa una condición del catálogo. Son las funciones JS del catálogo original, portadas una por una.
pub fn cond(src: &str, f: &Facts) -> bool {
    let (nv, amd) = (f.gpu_vendor == "NVIDIA", f.gpu_vendor == "AMD");
    match src {
        "() => true" => true,
        "s => !s.laptop" => !f.laptop,
        "s => s.laptop" => f.laptop,
        "s => s.gpuVendor === 'NVIDIA'" => nv,
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
    pub fn applies(&self, f: &Facts) -> bool { self.needs.as_deref().map_or(true, |s| cond(s, f)) }
    pub fn recommended(&self, f: &Facts) -> bool { self.applies(f) && self.rec.as_deref().map_or(false, |s| cond(s, f)) }
    pub fn reason(&self, f: &Facts) -> String { self.why.as_deref().map(|s| why(s, f)).unwrap_or_default() }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
