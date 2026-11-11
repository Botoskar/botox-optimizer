//! Reporte de diagnóstico para pegar en Discord o GitHub, y la página oficial de drivers de cada fabricante.
use serde_json::Value;

fn s(v: &Value) -> &str { v.as_str().unwrap_or_default() }

/// Encabezado con versión, Windows y hardware, más el registro de la consola, en un bloque de código.
/// Se pega en lugares públicos: se tapan el nombre de la PC y la carpeta del usuario (que lleva su nombre).
pub fn report(spec: Option<&Value>, admin: bool, fake: Option<&str>, lines: &[String], profile: &str, host: &str) -> String {
    let mut out = vec![format!("Botox Optimizer {}", env!("CARGO_PKG_VERSION")), format!("Fecha: {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))];
    if let Some(sp) = spec {
        let os = &sp["os"];
        out.push(format!("Windows: {} {} (build {})", s(&os["name"]), s(&os["ver"]), s(&os["build"])));
        out.push(format!("CPU: {}", s(&sp["cpu"]["name"])));
        let g = crate::sys::main_gpu(sp);
        out.push(format!("GPU: {} · driver {}", s(&g["name"]), s(&g["driver"])));
        out.push(format!("RAM: {} GB", sp["ram"]["gb"]));
    }
    out.push(format!("Administrador: {}", if admin { "sí" } else { "no" }));
    out.push(format!("Modo prueba: {}", fake.unwrap_or("no")));
    out.push(String::new());
    // Sin datos que identifiquen a la persona, en el registro (el encabezado no los tiene y lleva el nombre de la app).
    // Nombres vacíos o muy cortos no se reemplazan (taparían letras sueltas). El de la PC, solo como palabra entera:
    // una PC que se llama "Botox" no debe tapar "BotoxOptimizer".
    let mut log = lines.join("\n");
    for (secret, mask, word) in [(profile, "%USERPROFILE%", false), (host, "<PC>", true)] {
        if secret.len() >= 3 {
            log = replace_ci(&log, secret, mask, word);
        }
    }
    out.push(log);
    format!("```\n{}\n```", out.join("\n"))
}

/// Reemplazo sin distinguir mayúsculas (Windows escribe la misma ruta de las dos formas).
/// `word`: solo donde no lo rodean letras ni números.
fn replace_ci(text: &str, from: &str, to: &str, word: bool) -> String {
    // to_lowercase puede cambiar el largo de letras raras: si pasa, se busca solo lo exacto.
    let (lower, needle) = match (text.to_lowercase(), from.to_lowercase()) {
        (l, n) if l.len() == text.len() && n.len() == from.len() => (l, n),
        _ => (text.to_string(), from.to_string()),
    };
    let alnum = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, _) in lower.match_indices(&needle) {
        if i < last || word && (alnum(text[..i].chars().next_back()) || alnum(text[i + needle.len()..].chars().next())) {
            continue;
        }
        out.push_str(&text[last..i]);
        out.push_str(to);
        last = i + needle.len();
    }
    out.push_str(&text[last..]);
    out
}

/// Página oficial de descarga de drivers del fabricante de la placa de video.
pub fn driver_url(vendor: &str) -> Option<&'static str> {
    match vendor {
        "NVIDIA" => Some("https://www.nvidia.com/es-la/drivers/"),
        "AMD" => Some("https://www.amd.com/es/support/download/drivers.html"),
        "Intel" => Some("https://www.intel.com/content/www/us/en/support/detect.html"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_diagnostico_trae_lo_necesario_y_no_identifica_a_nadie() {
        let spec = json!({
            "pc": "PC-DE-JUAN",
            "os": {"name": "Windows 11 Pro", "ver": "24H2", "build": "26100"},
            "cpu": {"name": "AMD Ryzen 5 5600"},
            "gpus": [{"name": "NVIDIA GeForce RTX 4060", "driver": "32.0.15.6109", "vramGB": 8}],
            "ram": {"gb": 32}
        });
        let lines = vec![r"10:00:00 ERROR no se pudo leer C:\Users\Juan\AppData\Local\x".to_string(), "10:00:01 OK listo en pc-de-juan".to_string()];
        let r = report(Some(&spec), true, None, &lines, r"C:\Users\Juan", "PC-DE-JUAN");
        assert!(r.starts_with("```\nBotox Optimizer ") && r.ends_with("\n```"));
        for want in ["Windows 11 Pro 24H2 (build 26100)", "CPU: AMD Ryzen 5 5600", "GPU: NVIDIA GeForce RTX 4060 · driver 32.0.15.6109", "RAM: 32 GB", "Administrador: sí", "Modo prueba: no", r"%USERPROFILE%\AppData", "listo en <PC>"] {
            assert!(r.contains(want), "falta {want:?} en:\n{r}");
        }
        assert!(!r.to_lowercase().contains("juan"), "{r}");
        // Una PC que se llama como la app: se tapa el nombre suelto, no la marca ni sus rutas.
        let lines = vec![r"ERROR C:\ProgramData\BotoxOptimizer\backup\x.json en botox".to_string()];
        let r = report(None, true, None, &lines, r"C:\Users\Botox", "BOTOX");
        assert!(r.starts_with("```\nBotox Optimizer ") && r.contains(r"C:\ProgramData\BotoxOptimizer\backup") && r.contains("en <PC>"), "{r}");
        // Sin escaneo todavía, igual sale el encabezado.
        assert!(report(None, false, Some("amd"), &[], "", "").contains("Modo prueba: amd"));
    }

    #[test]
    fn drivers_solo_de_paginas_oficiales() {
        for v in ["NVIDIA", "AMD", "Intel"] {
            assert!(driver_url(v).is_some_and(|u| u.starts_with("https://www.") && !u.contains('&')), "{v}");
        }
        assert_eq!(driver_url("Otro"), None);
    }
}
