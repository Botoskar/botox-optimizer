//! Programas que arrancan con Windows.
//! Usa el mismo mecanismo que el Administrador de tareas (clave StartupApproved): apagar un programa no borra ni
//! renombra nada, solo lo marca como deshabilitado, y se vuelve a encender igual. Los apagados desde acá
//! también se ven apagados en el Administrador de tareas, y al revés.
use std::path::PathBuf;
use winreg::enums::*;
use winreg::{RegKey, RegValue, HKEY};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

/// De dónde sale el programa.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Place { UserRun, MachineRun, MachineRun32, UserFolder, CommonFolder }

impl Place {
    fn hive(self) -> HKEY {
        match self {
            Place::UserRun | Place::UserFolder => HKEY_CURRENT_USER,
            _ => HKEY_LOCAL_MACHINE,
        }
    }
    /// Clave donde Windows guarda si está habilitado.
    fn approved(self) -> String {
        let leaf = match self {
            Place::UserRun | Place::MachineRun => "Run",
            Place::MachineRun32 => "Run32",
            Place::UserFolder | Place::CommonFolder => "StartupFolder",
        };
        format!(r"{APPROVED}\{leaf}")
    }
    pub fn label(self) -> &'static str {
        match self {
            Place::UserRun => "Tu usuario",
            Place::MachineRun => "Todos los usuarios",
            Place::MachineRun32 => "Todos (32 bits)",
            Place::UserFolder => "Carpeta Inicio",
            Place::CommonFolder => "Carpeta Inicio (todos)",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    /// Nombre del valor del registro, o nombre del archivo en la carpeta Inicio.
    pub name: String,
    /// Comando que se ejecuta, o ruta del archivo.
    pub command: String,
    pub place: Place,
    pub enabled: bool,
}

/// Binario de 12 bytes que escribe Windows: 02 = habilitado, 03 = deshabilitado (y la hora en que se apagó).
fn approved_bytes(enabled: bool) -> Vec<u8> {
    let mut b = vec![if enabled { 2 } else { 3 }, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    if !enabled {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        b[4..12].copy_from_slice(&((secs + 11_644_473_600) * 10_000_000).to_le_bytes()); // FILETIME
    }
    b
}

/// Sin valor, Windows lo considera habilitado. Un primer byte impar (3, 7) es deshabilitado.
fn is_enabled(hive: HKEY, sub: &str, name: &str) -> bool {
    RegKey::predef(hive).open_subkey(sub).and_then(|k| k.get_raw_value(name)).map_or(true, |v| v.bytes.first().is_none_or(|b| b % 2 == 0))
}

fn set_enabled(hive: HKEY, sub: &str, name: &str, enabled: bool) -> Result<(), String> {
    let (key, _) = RegKey::predef(hive).create_subkey(sub).map_err(|e| e.to_string())?;
    key.set_raw_value(name, &RegValue { bytes: approved_bytes(enabled), vtype: REG_BINARY }).map_err(|e| e.to_string())
}

fn text(v: &RegValue) -> String {
    let w: Vec<u16> = v.bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()
}

fn folder(var: &str) -> Option<PathBuf> {
    let base = std::env::var_os(var)?;
    Some(PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
}

/// Lee todas las fuentes de arranque. Solo lee: no hace falta ser administrador.
pub fn scan() -> Vec<Item> {
    let mut v = Vec::new();
    for place in [Place::UserRun, Place::MachineRun, Place::MachineRun32] {
        let path = if place == Place::MachineRun32 { RUN32 } else { RUN };
        let Ok(key) = RegKey::predef(place.hive()).open_subkey(path) else { continue };
        let approved = place.approved();
        for (name, val) in key.enum_values().flatten() {
            v.push(Item { enabled: is_enabled(place.hive(), &approved, &name), command: text(&val), name, place });
        }
    }
    for (place, dir) in [(Place::UserFolder, folder("APPDATA")), (Place::CommonFolder, folder("ProgramData"))] {
        let Some(Ok(entries)) = dir.map(std::fs::read_dir) else { continue };
        let approved = place.approved();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // desktop.ini es un archivo oculto de Windows, no un programa.
            if name.eq_ignore_ascii_case("desktop.ini") || e.path().is_dir() {
                continue;
            }
            v.push(Item { enabled: is_enabled(place.hive(), &approved, &name), command: e.path().display().to_string(), name, place });
        }
    }
    v.sort_by_key(|i| i.name.to_lowercase());
    v
}

/// Enciende o apaga un programa del arranque. Reversible: solo cambia la marca de Windows.
pub fn set(i: &Item, enabled: bool) -> Result<(), String> {
    set_enabled(i.place.hive(), &i.place.approved(), &i.name, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUB: &str = r"Software\BotoxStartupTest\StartupApproved";

    /// Prueba en una clave propia de HKCU: no toca ningún programa real del arranque.
    #[test]
    fn apagar_y_encender_es_reversible() {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let _ = root.delete_subkey_all(r"Software\BotoxStartupTest");
        assert!(is_enabled(HKEY_CURRENT_USER, SUB, "x"), "sin valor = habilitado");
        set_enabled(HKEY_CURRENT_USER, SUB, "x", false).unwrap();
        assert!(!is_enabled(HKEY_CURRENT_USER, SUB, "x"));
        let raw = root.open_subkey(SUB).unwrap().get_raw_value("x").unwrap();
        assert_eq!((raw.bytes.len(), raw.bytes[0], raw.vtype), (12, 3, REG_BINARY));
        set_enabled(HKEY_CURRENT_USER, SUB, "x", true).unwrap();
        assert!(is_enabled(HKEY_CURRENT_USER, SUB, "x"));
        // Valores que escribe el Administrador de tareas: 06 habilitado, 07 deshabilitado.
        let (k, _) = root.create_subkey(SUB).unwrap();
        k.set_raw_value("t", &RegValue { bytes: vec![7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], vtype: REG_BINARY }).unwrap();
        k.set_raw_value("u", &RegValue { bytes: vec![6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], vtype: REG_BINARY }).unwrap();
        assert!(!is_enabled(HKEY_CURRENT_USER, SUB, "t") && is_enabled(HKEY_CURRENT_USER, SUB, "u"));
        let _ = root.delete_subkey_all(r"Software\BotoxStartupTest");
    }

    #[test]
    fn escanear_no_falla_ni_lista_desktop_ini() {
        let v = scan();
        assert!(v.iter().all(|i| !i.name.eq_ignore_ascii_case("desktop.ini") && !i.name.is_empty()));
    }
}
