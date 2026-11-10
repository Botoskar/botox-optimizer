//! Motor: aplica, revierte y chequea tweaks. Registro, servicios y energía van nativos;
//! lo que depende de cmdlets de Windows (apps, tareas, placa de red) corre sus scripts de PowerShell.
use crate::catalog::{Boost, Catalog, PwrEntry, Tweak};
use crate::reg;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::Instant;

const NO_WINDOW: u32 = 0x0800_0000;

/// Línea de la consola de actividad. lvl: start, info, ok, warn, cmd, out, endok, endwarn, enderr.
#[derive(Clone, Debug)]
pub struct Line { pub t: String, pub lvl: String, pub msg: String }

#[derive(Clone)]
pub struct Log(pub Option<Sender<Line>>);

impl Log {
    pub fn put(&self, lvl: &str, msg: impl Into<String>) {
        if let Some(tx) = &self.0 {
            let _ = tx.send(Line { t: chrono::Local::now().format("%H:%M:%S").to_string(), lvl: lvl.into(), msg: msg.into() });
        }
    }
}

pub fn is_admin() -> bool {
    #[link(name = "shell32")]
    extern "system" {
        fn IsUserAnAdmin() -> i32;
    }
    unsafe { IsUserAnAdmin() != 0 }
}

/// Corre un programa sin ventana. Devuelve (código de salida, salida estándar).
fn run(exe: &str, args: &[&str]) -> (i32, String) {
    match Command::new(exe).args(args).creation_flags(NO_WINDOW).stdin(Stdio::null()).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned()),
        Err(e) => (-1, e.to_string()),
    }
}

// ── Registro y servicios ──

fn start_names(v: i64) -> Option<(&'static str, &'static str)> {
    match v {
        2 => Some(("auto", "Automatic")),
        3 => Some(("demand", "Manual")),
        4 => Some(("disabled", "Disabled")),
        _ => None,
    }
}

/// Escribe un valor. El arranque de un servicio se cambia con el Administrador de servicios (sc.exe),
/// porque algunas claves de servicios solo permiten lectura a Administradores.
fn set_value(p: &str, n: &str, v: &Value, t: &str, log: &Log) -> Result<(), String> {
    let old = reg::get(p, n).map(|x| x.0);
    if let (Some(svc), Some((arg, new))) = (reg::service_of(p, n), v.as_i64().and_then(start_names)) {
        let (code, _) = run("sc.exe", &["config", svc, "start=", arg]);
        if code != 0 {
            return Err(format!("no se pudo cambiar el servicio {svc} (sc.exe devolvió {code}; 5 = acceso denegado)"));
        }
        let was = old.as_ref().and_then(Value::as_i64).and_then(start_names).map_or("?", |x| x.1);
        log.put("info", format!("Servicio {svc}: {was} → {new}"));
        return Ok(());
    }
    reg::set(p, n, v, t)?;
    let was = old.as_ref().map_or("(no existía)".to_string(), reg::fmt);
    log.put("info", format!("{}\\{n}: {was} → {}", reg::short(p), reg::fmt(v)));
    Ok(())
}

fn del_value(p: &str, n: &str, log: &Log) {
    if let Some((old, _)) = reg::get(p, n) {
        reg::del(p, n);
        log.put("info", format!("{}\\{n}: {} → (eliminado)", reg::short(p), reg::fmt(&old)));
    }
}

fn backup(id: &str, items: Vec<Value>, log: &Log) -> Result<(), String> {
    let n = items.len();
    if reg::save_backup(id, &Value::Array(items))? {
        log.put("info", format!("Respaldo guardado: {} ({n} valores)", reg::backup_path(id).display()));
    } else {
        log.put("info", format!("Ya hay un respaldo de '{id}': se conservan los valores originales"));
    }
    Ok(())
}

// ── Planes de energía ──

/// Valor actual (con corriente) de un ajuste del plan activo. Se lee del texto de powercfg sin depender del idioma.
pub fn pwr_get(sub: &str, set: &str) -> Option<i64> {
    let (_, out) = run("powercfg.exe", &["/qh", "SCHEME_CURRENT", sub, set]);
    let b = out.as_bytes();
    let hex: Vec<i64> = (0..b.len().saturating_sub(9))
        .filter(|&i| b[i] == b'0' && b[i + 1] == b'x' && b[i + 2..i + 10].iter().all(u8::is_ascii_hexdigit))
        .filter_map(|i| i64::from_str_radix(&out[i + 2..i + 10], 16).ok())
        .collect();
    // Los dos últimos son el valor con corriente (AC) y con batería (DC).
    (hex.len() >= 2).then(|| hex[hex.len() - 2])
}

fn pwr_name(set: &str) -> &str {
    match &set[..8.min(set.len())] {
        "48e6b7a6" => "Suspensión selectiva USB",
        "ee12f906" => "Ahorro de energía PCIe (ASPM)",
        "0cc5b647" => "Núcleos sin estacionar (%)",
        "be337238" => "Modo boost del procesador",
        "893dee8e" => "Estado mínimo del procesador (%)",
        "6738e2c4" => "Apagar disco tras (s)",
        _ => set,
    }
}

fn pwr_set(sub: &str, set: &str, v: i64, ac_only: bool, log: &Log) {
    let old = pwr_get(sub, set);
    let val = v.to_string();
    run("powercfg.exe", &["/setacvalueindex", "SCHEME_CURRENT", sub, set, &val]);
    if !ac_only {
        run("powercfg.exe", &["/setdcvalueindex", "SCHEME_CURRENT", sub, set, &val]);
    }
    let when = if ac_only { " (enchufada)" } else { "" };
    log.put("info", format!("Plan de energía{when} · {}: {} → {v}", pwr_name(set), old.map_or("?".into(), |o| o.to_string())));
}

// ── Tweaks nativos ──

fn live(t: &Tweak) -> impl Iterator<Item = &crate::catalog::RegEntry> { t.reg.iter().filter(|e| !e.svc || reg::key_exists(&e.p)) }

fn apply_native(t: &Tweak, log: &Log) -> Result<(), String> {
    if !t.reg.is_empty() {
        let items = live(t)
            .map(|e| {
                let cur = reg::get(&e.p, &e.n);
                json!({ "p": e.p, "n": e.n, "e": cur.is_some(), "v": cur.as_ref().map(|c| c.0.clone()), "t": cur.map(|c| c.1) })
            })
            .collect();
        backup(&t.id, items, log)?;
        for e in live(t) {
            set_value(&e.p, &e.n, &e.v, &e.t, log)?;
        }
        for e in live(t).filter(|e| e.svc && e.v.as_i64() == Some(4)) {
            let svc = reg::service_of(&e.p, &e.n).unwrap_or_default();
            if run("sc.exe", &["stop", svc]).0 == 0 {
                log.put("info", format!("Servicio {svc}: detenido"));
            }
        }
    }
    if !t.pwr.is_empty() {
        backup(&t.id, t.pwr.iter().map(|x| json!({ "s": x.sub, "k": x.set, "v": pwr_get(&x.sub, &x.set) })).collect(), log)?;
        for x in &t.pwr {
            pwr_set(&x.sub, &x.set, x.v, t.ac, log);
        }
        run("powercfg.exe", &["/setactive", "SCHEME_CURRENT"]);
    }
    Ok(())
}

fn revert_native(t: &Tweak, log: &Log) -> Result<(), String> {
    let bk = reg::take_backup(&t.id);
    match &bk {
        Some(_) => log.put("info", format!("Restaurando desde el respaldo {}", reg::backup_path(&t.id).display())),
        None => log.put("warn", "Sin respaldo: se vuelve a los valores por defecto de Windows"),
    }
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    if !t.reg.is_empty() {
        let mut done = HashSet::new();
        for i in bk.iter().flatten() {
            let (p, n) = (s(i, "p"), s(i, "n"));
            if i["e"].as_bool() == Some(true) {
                set_value(&p, &n, &i["v"], i["t"].as_str().unwrap_or("DWord"), log)?;
            } else {
                del_value(&p, &n, log);
            }
            done.insert((p, n));
        }
        // Lo que no está en el respaldo (respaldo viejo o incompleto) vuelve al valor por defecto de Windows.
        for e in live(t).filter(|e| !done.contains(&(e.p.clone(), e.n.clone()))) {
            if e.def.is_null() { del_value(&e.p, &e.n, log) } else { set_value(&e.p, &e.n, &e.def, &e.t, log)? }
        }
    }
    if !t.pwr.is_empty() {
        match &bk {
            Some(items) => {
                for i in items {
                    if let Some(v) = i["v"].as_i64() {
                        pwr_set(&s(i, "s"), &s(i, "k"), v, t.ac, log);
                    }
                }
            }
            None => t.pwr.iter().for_each(|x: &PwrEntry| pwr_set(&x.sub, &x.set, x.def, t.ac, log)),
        }
        run("powercfg.exe", &["/setactive", "SCHEME_CURRENT"]);
    }
    Ok(())
}

pub fn check_native(t: &Tweak) -> bool {
    let reg_ok = live(t).all(|e| reg::get(&e.p, &e.n).is_some_and(|c| reg::fmt(&c.0) == reg::fmt(&e.v)));
    reg_ok && t.pwr.iter().all(|x| pwr_get(&x.sub, &x.set) == Some(x.v))
}

// ── PowerShell ──

pub struct PsOut { pub ok: bool, pub out: String, pub err: String }

/// Corre un script con el preludio del catálogo. Las líneas `LOG:nivel|mensaje` y la salida van en vivo a la consola.
pub fn run_ps(prelude: &str, script: &str, log: &Log) -> PsOut {
    let fail = |e: String| PsOut { ok: false, out: String::new(), err: e };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let file = std::env::temp_dir().join(format!("botox-{}-{nanos:x}.ps1", std::process::id()));
    // BOM: PowerShell 5.1 lee el archivo como UTF-8 solo si lo trae.
    let body = format!("\u{feff}{prelude}\ntry {{\n{script}\n}} catch {{ Write-Output ('ERR:' + $_.Exception.Message); exit 1 }}");
    if let Err(e) = std::fs::write(&file, body) {
        return fail(e.to_string());
    }
    let child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&file)
        .creation_flags(NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return fail(format!("no se pudo iniciar PowerShell: {e}")),
    };
    // stderr se lee en otro hilo para que ninguno de los dos tubos se llene y trabe el proceso.
    let mut stderr = child.stderr.take().unwrap();
    let errlog = log.clone();
    let errs = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stderr.read_to_end(&mut s);
        let s = String::from_utf8_lossy(&s).into_owned();
        s.lines().filter(|l| !l.trim().is_empty()).for_each(|l| errlog.put("warn", l.trim()));
        s
    });
    let (mut out, mut reader, mut raw) = (String::new(), BufReader::new(child.stdout.take().unwrap()), Vec::new());
    while reader.read_until(b'\n', &mut raw).map_or(false, |n| n > 0) {
        let text = String::from_utf8_lossy(&raw).into_owned();
        raw.clear();
        let line = text.trim_end_matches(['\r', '\n']);
        if let Some(rest) = line.strip_prefix("LOG:") {
            if let Some((lvl, msg)) = rest.split_once('|') {
                log.put(lvl, msg);
                continue;
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with("ERR:") {
            log.put("out", line.trim());
        }
        out.push_str(line);
        out.push('\n');
    }
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
    let stderr = errs.join().unwrap_or_default();
    let _ = std::fs::remove_file(&file);
    let out = out.trim().to_string();
    let caught = out.split_once("ERR:").map(|x| x.1.trim().to_string());
    let err = caught.clone().unwrap_or_else(|| if code != 0 { if stderr.trim().is_empty() { format!("PowerShell terminó con código {code}") } else { stderr.trim().to_string() } } else { String::new() });
    PsOut { ok: code == 0 && caught.is_none(), out, err }
}

// ── Operaciones que usa la interfaz ──

pub fn check(cat: &Catalog, t: &Tweak) -> bool {
    match &t.ps {
        None => check_native(t),
        Some(ps) => run_ps(&cat.prelude, &ps.check, &Log(None)).out.trim_end().ends_with("True"),
    }
}

/// Estado de todos los tweaks que usan PowerShell.
/// Corren en 4 procesos a la vez, agrupados por lo que consultan (apps, tareas, red, resto):
/// cada grupo carga su parte lenta de Windows una sola vez y el total tarda lo que el grupo más lento.
pub fn check_all_ps(cat: &Catalog) -> HashMap<String, bool> {
    let group = |check: &str| {
        if check.contains("AppxCheck") { 0 } else if check.contains("TaskCheck") { 1 } else if check.contains("Nic") { 2 } else { 3 }
    };
    let mut scripts = vec![String::from("$r=[ordered]@{}\n"); 4];
    for t in cat.tweaks.iter().filter(|t| !t.native) {
        let check = &t.ps.as_ref().unwrap().check;
        scripts[group(check)] += &format!("try {{ $r['{0}'] = & {{ {check} }} }} catch {{ $r['{0}'] = $false }}\n", t.id);
    }
    std::thread::scope(|s| {
        let jobs: Vec<_> = scripts
            .iter()
            .map(|script| {
                s.spawn(move || {
                    let out = run_ps(&cat.prelude, &format!("{script}$r | ConvertTo-Json -Compress"), &Log(None)).out;
                    // Un chequeo que devuelve algo raro (lista, vacío) cuenta como "no aplicado" en vez de tirar todo el grupo.
                    let map: HashMap<String, serde_json::Value> = out.find('{').and_then(|i| serde_json::from_str(&out[i..]).ok()).unwrap_or_default();
                    map.into_iter().map(|(k, v)| (k, v.as_bool().unwrap_or(false))).collect::<HashMap<_, _>>()
                })
            })
            .collect();
        let mut all: HashMap<String, bool> = jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect();
        // Si un grupo entero falló, sus tweaks igual reciben estado: sin estado, el interruptor queda "cargando" y no se puede tocar.
        for t in cat.tweaks.iter().filter(|t| !t.native) {
            all.entry(t.id.clone()).or_insert(false);
        }
        all
    })
}

pub fn check_all_native(cat: &Catalog) -> HashMap<String, bool> {
    cat.tweaks.iter().filter(|t| t.native).map(|t| (t.id.clone(), check_native(t))).collect()
}

/// Cambiar el registro no alcanza: el Explorador y otras apps leen sus ajustes al iniciar.
/// Esto les avisa que cambió la configuración y refresca las ventanas abiertas del Explorador.
pub fn refresh_shell() {
    use std::ffi::c_void;
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(event: i32, flags: u32, a: *const c_void, b: *const c_void);
    }
    #[link(name = "user32")]
    extern "system" {
        fn SendMessageTimeoutW(hwnd: isize, msg: u32, w: usize, l: *const u16, flags: u32, timeout: u32, result: *mut usize) -> isize;
        fn FindWindowExW(parent: isize, after: isize, class: *const u16, title: *const u16) -> isize;
        fn PostMessageW(hwnd: isize, msg: u32, w: usize, l: isize) -> i32;
    }
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    const HWND_BROADCAST: isize = 0xFFFF;
    const WM_SETTINGCHANGE: u32 = 0x001A;
    const WM_COMMAND: u32 = 0x0111;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    const EXPLORER_REFRESH: usize = 41504; // el comando "Actualizar" (F5) de una ventana del Explorador
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null());
        let (mut out, area) = (0usize, wide("ShellState"));
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, area.as_ptr(), SMTO_ABORTIFHUNG, 1000, &mut out);
        let class = wide("CabinetWClass");
        let mut hwnd = FindWindowExW(0, 0, class.as_ptr(), std::ptr::null());
        while hwnd != 0 {
            PostMessageW(hwnd, WM_COMMAND, EXPLORER_REFRESH, 0);
            hwnd = FindWindowExW(0, hwnd, class.as_ptr(), std::ptr::null());
        }
    }
}

fn secs(t0: Instant) -> String { format!("{:.1}", t0.elapsed().as_secs_f32()).replace('.', ",") }

pub struct Done { pub ok: bool, pub state: bool, pub err: String }

/// Aplica (on) o revierte un tweak y verifica el estado real. Deja todo registrado en la consola.
pub fn toggle(cat: &Catalog, t: &Tweak, on: bool, log: &Log) -> Done {
    let t0 = Instant::now();
    log.put("start", format!("{}: {}", if on { "Aplicar" } else { "Revertir" }, t.name));
    let res = match &t.ps {
        None => if on { apply_native(t, log) } else { revert_native(t, log) },
        Some(ps) => {
            let o = run_ps(&cat.prelude, if on { &ps.apply } else { &ps.revert }, log);
            if o.ok { Ok(()) } else { Err(o.err) }
        }
    };
    if res.is_ok() {
        refresh_shell();
        log.put("info", "Avisé a Windows y al Explorador para que tomen el cambio sin reiniciar");
    }
    log.put("info", "Verificando el estado real en Windows…");
    let state = check(cat, t);
    match &res {
        Err(e) => log.put("enderr", format!("Error: {e} ({} s)", secs(t0))),
        Ok(()) if state == on => {
            let reboot = if on && t.reboot { " · reiniciá para que tome efecto" } else { "" };
            log.put("endok", format!("Listo en {} s · verificado: {}{reboot}", secs(t0), if on { "activo" } else { "revertido" }));
        }
        Ok(()) => log.put("endwarn", format!("Listo en {} s · Windows todavía no refleja el cambio (puede requerir reinicio)", secs(t0))),
    }
    Done { ok: res.is_ok(), state, err: res.err().unwrap_or_default() }
}

/// Corre una limpieza de Boost-Up. Devuelve (ok, resultado, error).
pub fn boost(cat: &Catalog, b: &Boost, log: &Log) -> (bool, String, String) {
    let t0 = Instant::now();
    log.put("start", b.name.clone());
    let o = run_ps(&cat.prelude, &b.ps, log);
    if o.ok {
        log.put("endok", format!("Listo en {} s", secs(t0)));
    } else {
        log.put("enderr", format!("Error: {} ({} s)", o.err, secs(t0)));
    }
    (o.ok, o.out.lines().last().unwrap_or_default().to_string(), o.err)
}

/// Puntos de restauración: (descripción, fecha). Err si no se pudieron listar (hace falta administrador).
pub fn restore_points(cat: &Catalog) -> Result<Vec<(String, String)>, String> {
    let o = run_ps(&cat.prelude, "@(Get-ComputerRestorePoint | Sort-Object SequenceNumber -Descending | ForEach-Object { [ordered]@{ d = $_.Description; t = $_.ConvertToDateTime($_.CreationTime).ToString('yyyy-MM-dd HH:mm') } }) | ConvertTo-Json -Compress", &Log(None));
    if !o.ok {
        return Err(o.err);
    }
    let v: Value = o.out.find(['[', '{']).and_then(|i| serde_json::from_str(&o.out[i..]).ok()).unwrap_or(Value::Null);
    let list = match v { Value::Array(a) => a, Value::Null => vec![], one => vec![one] };
    Ok(list.iter().map(|x| (x["d"].as_str().unwrap_or_default().to_string(), x["t"].as_str().unwrap_or_default().to_string())).collect())
}

pub fn create_restore_point(cat: &Catalog, log: &Log) -> Result<(), String> {
    let t0 = Instant::now();
    log.put("start", "Crear punto de restauración");
    let o = run_ps(&cat.prelude, "Log 'Pidiéndole a Windows un punto de restauración (puede tardar un minuto)…'; Checkpoint-Computer -Description 'Botox Optimizer' -RestorePointType MODIFY_SETTINGS -WarningAction Stop; 'Punto de restauración creado'", log);
    if o.ok {
        log.put("endok", format!("Listo en {} s", secs(t0)));
        Ok(())
    } else {
        log.put("enderr", format!("Error: {} ({} s)", o.err, secs(t0)));
        Err(o.err)
    }
}

pub fn open_system_restore() {
    let _ = Command::new(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()) + "\\System32\\rstrui.exe").spawn();
}

/// Ids de tweaks con respaldo guardado y su fecha.
pub fn backups() -> Vec<(String, std::time::SystemTime)> {
    let Ok(dir) = std::fs::read_dir(reg::backup_dir()) else { return vec![] };
    dir.flatten()
        .filter_map(|f| {
            let name = f.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?.to_string();
            Some((id, f.metadata().ok()?.modified().ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::RegEntry;

    const KEY: &str = r"HKCU:\Software\BotoxOptimizerTest";

    fn entry(n: &str, v: Value, t: &str, def: Value) -> RegEntry { RegEntry { p: KEY.into(), n: n.into(), v, t: t.into(), def, svc: false } }

    /// Aplica y revierte un tweak de verdad sobre una clave de prueba propia, y deja todo como estaba.
    #[test]
    fn aplicar_y_revertir_restaura_los_valores_originales() {
        let dir = std::env::temp_dir().join(format!("botox-test-{}", std::process::id()));
        std::env::set_var("BX_BACKUP_DIR", &dir);
        let log = Log(None);
        // Estado original: A=1, S="x", B no existe.
        reg::set(KEY, "A", &json!(1), "DWord").unwrap();
        reg::set(KEY, "S", &json!("x"), "String").unwrap();
        reg::del(KEY, "B");
        let cat = Catalog::load();
        let mut t = cat.tweaks.iter().find(|t| t.native).unwrap().clone();
        t.id = "test-tweak".into();
        t.pwr.clear();
        t.reg = vec![entry("A", json!(5), "DWord", json!(0)), entry("S", json!("y"), "String", Value::Null), entry("B", json!([1, 2, 255]), "Binary", Value::Null)];

        assert!(!check_native(&t));
        apply_native(&t, &log).unwrap();
        assert!(check_native(&t), "después de aplicar tiene que quedar activo");
        assert!(reg::backup_path("test-tweak").exists(), "tiene que existir el respaldo");
        // Aplicar de nuevo no pisa el respaldo original.
        apply_native(&t, &log).unwrap();

        revert_native(&t, &log).unwrap();
        assert_eq!(reg::get(KEY, "A").unwrap().0, json!(1), "A vuelve al valor original, no al por defecto");
        assert_eq!(reg::get(KEY, "S").unwrap().0, json!("x"));
        assert!(reg::get(KEY, "B").is_none(), "B no existía: se borra");
        assert!(!reg::backup_path("test-tweak").exists(), "el respaldo se consume al revertir");

        // Sin respaldo, revertir usa los valores por defecto del catálogo.
        apply_native(&t, &log).unwrap();
        let _ = std::fs::remove_file(reg::backup_path("test-tweak"));
        revert_native(&t, &log).unwrap();
        assert_eq!(reg::get(KEY, "A").unwrap().0, json!(0));
        assert!(reg::get(KEY, "S").is_none());

        let _ = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).delete_subkey_all(r"Software\BotoxOptimizerTest");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lee_ajustes_de_energia() {
        // Suspensión selectiva USB: existe en todo Windows. Solo lectura.
        assert!(pwr_get("2a737441-1930-4402-8d77-b2bebba308a3", "48e6b7a6-50f5-4782-a5d4-53bb8f07e226").is_some());
    }

    #[test]
    fn powershell_transmite_log_y_errores() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cat = Catalog::load();
        let o = run_ps(&cat.prelude, "Log 'hola ñ' 'ok'; 'salida'", &Log(Some(tx.clone())));
        assert!(o.ok && o.out == "salida", "{} / {}", o.out, o.err);
        let lines: Vec<Line> = rx.try_iter().collect();
        assert!(lines.iter().any(|l| l.lvl == "ok" && l.msg == "hola ñ"), "{lines:?}");
        let o = run_ps(&cat.prelude, "throw 'falla de prueba'", &Log(None));
        assert!(!o.ok && o.err.contains("falla de prueba"), "{}", o.err);
    }
}

#[cfg(test)]
mod shell_tests {
    /// Solo manda avisos (equivale a apretar F5 en el Explorador): no cambia ninguna configuración.
    #[test]
    fn avisar_al_explorador_no_falla() { super::refresh_shell(); }
}
