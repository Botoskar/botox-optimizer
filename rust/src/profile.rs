//! Perfil portable (.botox): la lista de optimizaciones activas, para volver a aplicarlas después de formatear o en
//! otra PC. Lleva solo ids del catálogo; nunca valores de registro ni respaldos (no se restaura nada de otra PC).
use crate::catalog::Catalog;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Versión de la app que lo generó.
    pub version: String,
    pub applied: Vec<String>,
    /// Segundos Unix.
    pub saved_at: u64,
    /// "AMD Ryzen 5 5600 + NVIDIA GeForce RTX 4060, 32 GB", para saber de qué PC viene.
    pub pc_summary: String,
}

/// Un perfil real pesa unos pocos KB: lo que pase de esto no es un perfil.
const MAX_BYTES: u64 = 256 * 1024;

pub fn export(p: &Profile, path: &Path) -> Result<(), String> {
    std::fs::write(path, serde_json::to_string_pretty(p).unwrap()).map_err(|e| format!("no se pudo guardar: {e}"))
}

/// Lee y valida un perfil. Es un archivo de afuera: tamaño acotado, campos exactos, ids de este catálogo.
/// Devuelve el perfil con solo los ids conocidos (sin repetir) y cuántos se descartaron por desconocidos.
pub fn import(path: &Path, cat: &Catalog) -> Result<(Profile, usize), String> {
    let len = std::fs::metadata(path).map_err(|e| format!("no se pudo abrir: {e}"))?.len();
    if len > MAX_BYTES {
        return Err("el archivo es demasiado grande para ser un perfil".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("no se pudo leer: {e}"))?;
    let mut p: Profile = serde_json::from_str(&text).map_err(|e| format!("no es un perfil de Botox Optimizer válido ({e})"))?;
    let total = p.applied.len();
    let mut seen = std::collections::HashSet::new();
    p.applied.retain(|id| cat.tweak(id).is_some() && seen.insert(id.clone()));
    let dropped = total - p.applied.len();
    Ok((p, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf { std::env::temp_dir().join(format!("bx-profile-{}-{name}", std::process::id())) }

    #[test]
    fn exportar_e_importar_da_lo_mismo() {
        let cat = Catalog::load();
        let p = Profile { version: "2.3.1".into(), applied: vec!["gamemode".into(), "hags".into()], saved_at: 1, pc_summary: "Ryzen + RTX".into() };
        let f = tmp("ok.botox");
        export(&p, &f).unwrap();
        assert_eq!(import(&f, &cat).unwrap(), (p, 0));
        let _ = std::fs::remove_file(f);
    }

    #[test]
    fn rechaza_lo_que_no_es_un_perfil() {
        let cat = Catalog::load();
        let f = tmp("bad.botox");
        // Un campo de más (p. ej. alguien intentando colar valores de registro) no se ignora en silencio.
        std::fs::write(&f, r#"{"version":"1","applied":[],"saved_at":0,"pc_summary":"","reg":{"x":1}}"#).unwrap();
        assert!(import(&f, &cat).unwrap_err().contains("no es un perfil"));
        std::fs::write(&f, "no es json").unwrap();
        assert!(import(&f, &cat).is_err());
        std::fs::write(&f, vec![b' '; (MAX_BYTES + 1) as usize]).unwrap();
        assert!(import(&f, &cat).unwrap_err().contains("demasiado grande"));
        // Ids que este catálogo no conoce (o repetidos) se descartan y se cuentan.
        std::fs::write(&f, r#"{"version":"9","applied":["gamemode","gamemode","no-existe","..\\x"],"saved_at":0,"pc_summary":""}"#).unwrap();
        let (p, dropped) = import(&f, &cat).unwrap();
        assert_eq!((p.applied, dropped), (vec!["gamemode".to_string()], 3));
        let _ = std::fs::remove_file(f);
    }
}
