#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod catalog;
mod diag;
mod engine;
mod hwscan;
mod pages;
mod profile;
mod reg;
mod sha256;
mod startup;
mod sys;
mod theme;
mod update;
mod win;

use catalog::{Catalog, Facts};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, RichText, Sense, Vec2};
use egui_phosphor::regular as ic;
use engine::{Line, Log};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use theme::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page { Home, Scan, Tweaks, Debloat, Boost, Startup, Backup }

const NAV: [(Page, &str, &str); 7] = [
    (Page::Home, ic::HOUSE, "Inicio"),
    (Page::Scan, ic::CPU, "Escáner"),
    (Page::Tweaks, ic::ROCKET_LAUNCH, "Optimizaciones"),
    (Page::Debloat, ic::TRASH, "Debloat"),
    (Page::Boost, ic::GAUGE, "Boost-Up"),
    (Page::Startup, ic::POWER, "Arranque"),
    (Page::Backup, ic::CLOCK_COUNTER_CLOCKWISE, "Backups"),
];

/// Resultados que mandan los hilos de trabajo a la interfaz.
pub enum Msg {
    Scan(Result<Value, String>),
    /// Estados de tweaks. `ps` = es la tanda de PowerShell (la lenta).
    /// `na`: los que no aplican en esta PC (solo los trae la tanda de PowerShell).
    States { map: HashMap<String, bool>, ps: bool, na: HashSet<String> },
    Toggled { id: String, on: bool, done: engine::Done, quiet: bool },
    /// Re-chequeo de un solo tweak al pasar el mouse por su interruptor. `None` = no aplica en esta PC.
    Rechecked(String, Option<bool>),
    Boosted { id: String, ok: bool, out: String, err: String },
    Restore(Result<Vec<(String, String)>, String>),
    RestoreCreated(Result<(), String>),
    Stats(sys::Stats, (u64, u64)),
    /// Resultado de buscar una versión nueva al abrir.
    Update(Result<Option<update::Update>, String>),
    /// Resultado de bajar e instalar la versión nueva.
    Updated(Result<(), String>),
}

/// Lo que el usuario pidió durante el dibujado; se ejecuta al terminar el frame.
#[derive(Clone)]
pub enum Action {
    Go(Page, Option<&'static str>),
    Toggle(String),
    ToggleNow(String),
    Many(Vec<String>, &'static str),
    ManyNow(Vec<String>),
    RunBoost(String),
    RunBoostNow(String),
    Reboot,
    RebootNow,
    Rescan,
    /// Detiene los PowerShell en curso (chequeos, tweaks o limpiezas).
    Cancel,
    CreateRestore,
    OpenRestore,
    RevertAll,
    /// Enciende o apaga el programa de arranque que está en esa posición de la lista.
    Startup(usize),
    RescanStartup,
    /// Vuelve a leer el estado de los tweaks sin borrar el que se muestra (no reescanea el hardware).
    Verify,
    /// Elige un perfil .botox y muestra la confirmación de lote con lo que aplica a esta PC.
    ImportProfile,
    /// Pide confirmación para instalar la versión nueva; UpdateNow la instala.
    Update,
    UpdateNow,
}

pub struct Confirm { pub text: String, pub action: Action }

/// Aviso abajo a la derecha. `undo`: botón "Deshacer" que vuelve atrás el cambio recién hecho.
struct Toast { msg: String, color: Color32, at: Instant, secs: f32, undo: Option<Action> }

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Saved {
    /// Tweaks activados desde la app (para "Revertir todo" de los que no dejan respaldo).
    applied: HashSet<String>,
    /// Última vez que se corrió cada boost (segundos Unix).
    boost_last: HashMap<String, u64>,
    console_open: bool,
    /// Último escaneo y estados conocidos: se muestran al abrir mientras se actualizan de fondo.
    spec: Option<Value>,
    state: HashMap<String, bool>,
    /// Posición y tamaño de la ventana [x, y, ancho, alto] para abrirla donde quedó.
    win: Option<[f32; 4]>,
    maximized: bool,
}

const SIDE_W: f32 = 196.0;

pub struct App {
    pub cat: Arc<Catalog>,
    pub page: Page,
    pub tab: String,
    pub query: String,
    pub admin: bool,
    pub spec: Option<Value>,
    pub scan_err: Option<String>,
    pub facts: Facts,
    pub state: HashMap<String, bool>,
    /// Ya llegó la tanda lenta de estados (PowerShell).
    pub ps_ready: bool,
    pub busy: HashSet<String>,
    /// Se aplicó o revirtió algo que necesita reiniciar la PC.
    pub reboot_pending: bool,
    pub actions: Vec<Action>,
    pub confirm: Option<Confirm>,
    /// Tweaks que el usuario destildó en la confirmación de un lote.
    confirm_skip: HashSet<String>,
    queue: VecDeque<String>,
    bulk: Option<(usize, usize, bool, Vec<String>)>, // (total, ok, alguno pide reinicio, los que quedaron como se pidió)
    pub boost_sel: usize,
    pub boost_out: HashMap<String, String>,
    pub restore: Option<Result<Vec<(String, String)>, String>>,
    pub backups: Vec<(String, std::time::SystemTime)>,
    pub stats: sys::Stats,
    cpu_prev: (u64, u64),
    stats_at: Option<Instant>,
    stats_pending: bool,
    saved: Saved,
    lines: Vec<Line>,
    running: u32,
    run_start: Instant,
    toasts: Vec<Toast>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    log_tx: Sender<Line>,
    log_rx: Receiver<Line>,
    pub logo: egui::TextureHandle,
    /// Imagen propia elegida por el usuario; si no hay, se usa el logo.
    pub avatar: Option<egui::TextureHandle>,
    /// Programas que arrancan con Windows (se lee al entrar a la sección).
    pub startup: Vec<startup::Item>,
    /// Versión nueva disponible en GitHub, y si se está instalando.
    pub update: Option<update::Update>,
    /// Modo de prueba (BX_FAKE): hardware AMD simulado. No deja aplicar nada en la PC.
    pub fake: Option<String>,
    pub updating: bool,
    frames: u32,
    /// Cuándo se cambió de página (para el fundido de entrada).
    page_at: Instant,
    /// Ctrl+F: enfocar el buscador cuando se dibuje.
    pub focus_search: bool,
    /// "Optimizar para juegos" incluye también las de riesgo medio (casilla en Inicio).
    pub one_click_medium: bool,
    /// Cuándo terminó la última lectura completa del estado de los tweaks, y si hay una en curso.
    pub states_at: Option<Instant>,
    pub verifying: bool,
    /// La ventana tenía el foco en el cuadro anterior (para re-verificar al volver a ella).
    had_focus: bool,
    /// `--safe`: modo de recuperación, solo se puede revertir.
    pub safe: bool,
    /// Tweak cuyo detalle ("Qué cambia") se muestra en un panel.
    pub detail: Option<String>,
    /// Re-chequeo por hover: si hay uno en curso y cuándo se re-chequeó cada tweak.
    pub rechecking: bool,
    pub rechecked: HashMap<String, Instant>,
}

/// Re-chequear un tweak al pasar el mouse: solo si el estado tiene más de 5 min, no se re-chequeó ese mismo en los
/// últimos 5 min y no hay otro en curso (pasar el mouse por toda la lista no abre un PowerShell por interruptor).
fn should_recheck(states_age: Option<Duration>, last: Option<Duration>, in_flight: bool) -> bool {
    let old = |d: Option<Duration>| d.map_or(true, |d| d > Duration::from_secs(300));
    !in_flight && states_age.is_some() && old(states_age) && old(last)
}

fn main() -> eframe::Result {
    // El actualizador corre `exe --version` antes de instalar: tiene que contestar sin abrir nada más.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    install_panic_log();
    // Modo de prueba, solo lectura: imprime el estado de todos los tweaks como JSON y sale.
    if std::env::args().any(|a| a == "--check") {
        let cat = Catalog::load();
        let t0 = Instant::now();
        let mut all = engine::check_all_native(&cat);
        let native_ms = t0.elapsed().as_millis();
        all.extend(engine::check_all_ps(&cat).0);
        eprintln!("nativos: {native_ms} ms · total: {} ms · admin: {}", t0.elapsed().as_millis(), engine::is_admin());
        let sorted: std::collections::BTreeMap<_, _> = all.into_iter().collect();
        println!("{}", serde_json::to_string(&sorted).unwrap());
        return Ok(());
    }
    // Modo de prueba, solo lectura: imprime el escaneo de hardware como JSON y sale. `--scan ps` usa el script de PowerShell.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--scan") {
        let t0 = Instant::now();
        let spec = if args.get(i + 1).is_some_and(|a| a == "ps") { sys::scan_ps(&Catalog::load()) } else { hwscan::scan() };
        eprintln!("escaneo: {} ms", t0.elapsed().as_millis());
        println!("{}", spec.map_or_else(|e| format!("{{\"error\":{}}}", Value::String(e)), |v| v.to_string()));
        return Ok(());
    }
    // Emergencia sin interfaz: revierte todo lo que hizo la app (por si la ventana no abre). Va antes de on_start: una
    // corrida sin ventana justo después de actualizar dejaría el marcador en "1" y el próximo arranque volvería atrás.
    if args.iter().any(|a| a == "--revert-all") {
        if sys::fake_kind().is_some() {
            win::message("En modo prueba (BX_FAKE) no se revierte nada.", true);
        } else if !win::single_instance() {
            win::message("Botox Optimizer está abierto: cerralo y repetí, o usá \"Revertir todo\" en Backups.", true);
        } else {
            // Si falla, cada respaldo ilegible lo dice en el resumen.
            let _ = reg::repair_backups();
            let (ok, msg) = revert_all_cli(args.iter().any(|a| a == "--yes"));
            win::message(&msg, !ok);
        }
        return Ok(());
    }
    // Las copias de prueba (BX_FAKE, capturas) no cambian nada en la PC: pueden abrirse junto a la app real y no tocan
    // la actualización. La real primero toma el mutex: así otra instancia (o la vieja que todavía cierra) no la hace
    // salir con el marcador ya en "1", ni dos arranques juntos deciden volver atrás.
    if !is_test_copy() {
        if !win::single_instance() {
            return Ok(());
        }
        // Restaura la versión anterior si la nueva no llegó a arrancar bien (puede no volver: abre la vieja y sale).
        update::on_start();
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).expect("icono");
    let saved = load_saved();
    let mut vp = egui::ViewportBuilder::default()
        .with_title(match sys::fake_kind().as_deref() { Some("amd") => "Botox Optimizer · PRUEBA: Radeon + Ryzen simulados", Some(_) => "Botox Optimizer · PRUEBA: Ryzen X3D simulado", None => "Botox Optimizer" })
        .with_inner_size([1340.0, 860.0])
        .with_min_inner_size([1180.0, 720.0])
        .with_icon(icon)
        .with_maximized(saved.maximized);
    // Abre donde quedó la última vez (si no estaba minimizada fuera de pantalla).
    if let Some([x, y, w, h]) = saved.win.filter(|w| w[0] > -2000.0 && w[1] > -2000.0) {
        vp = vp.with_position([x, y]).with_inner_size([w, h]);
    }
    let opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native("Botox Optimizer", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

/// En release la app no tiene consola: un panic la cerraría sin dejar rastro. Se anota en
/// %LOCALAPPDATA%\BotoxOptimizer\crash.log la fecha, la versión, el mensaje, dónde pasó y el backtrace
/// (rutas de código y funciones; no datos del usuario).
/// Copia de prueba (BX_FAKE o captura): sin mutex ni confirmación/vuelta atrás de actualizaciones.
fn is_test_copy() -> bool {
    sys::fake_kind().is_some() || win::test_env("BX_SHOT").is_some()
}

fn install_panic_log() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let p = saved_path().with_file_name("crash.log");
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        // Un log que crece sin límite no ayuda: pasado 1 MB se empieza de nuevo.
        let append = std::fs::metadata(&p).is_ok_and(|m| m.len() < 1 << 20);
        let text = format!("--- {} · v{} · unix {}\n{info}\n{}\n", if engine::is_admin() { "admin" } else { "sin admin" }, env!("CARGO_PKG_VERSION"), unix_now(), std::backtrace::Backtrace::force_capture());
        let file = std::fs::OpenOptions::new().create(true).append(append).write(true).truncate(!append).open(&p);
        let _ = file.and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
        prev(info);
    }));
}

/// Tweaks activos que la app puede revertir: tienen respaldo o se activaron desde acá. Lo usan "Revertir todo" y
/// `--revert-all`.
fn reversible_ids(cat: &Catalog, is_on: &dyn Fn(&str) -> bool, backups: &[(String, std::time::SystemTime)], applied: &HashSet<String>) -> Vec<String> {
    let mut ids: Vec<String> = backups.iter().map(|b| b.0.trim_end_matches("-nic").to_string()).chain(applied.iter().cloned()).filter(|id| is_on(id) && cat.tweak(id).is_some()).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Modo seguro (`--safe`): solo revertir, crear o abrir puntos de restauración, mirar y reiniciar. Un interruptor solo
/// se puede tocar si está encendido (o sea, para revertirlo).
fn safe_allows(a: &Action, is_on: &dyn Fn(&str) -> bool) -> bool {
    match a {
        Action::Go(p, _) => *p == Page::Backup,
        Action::Toggle(id) | Action::ToggleNow(id) => is_on(id),
        Action::RevertAll | Action::ManyNow(_) | Action::CreateRestore | Action::OpenRestore | Action::Cancel | Action::Reboot | Action::RebootNow | Action::Rescan | Action::Verify => true,
        _ => false,
    }
}

/// `--revert-all`: lee el estado real, revierte todo lo reversible y deja un resumen. Sin `--yes` solo dice qué haría.
fn revert_all_cli(yes: bool) -> (bool, String) {
    if !engine::is_admin() {
        return (false, "Para revertir hay que abrirlo como administrador.".into());
    }
    let cat = Catalog::load();
    let mut state = engine::check_all_native(&cat);
    state.extend(engine::check_all_ps(&cat).0);
    let mut saved = load_saved();
    let ids = reversible_ids(&cat, &|id| state.get(id) == Some(&true), &engine::backups(), &saved.applied);
    if ids.is_empty() {
        return (true, "No hay cambios de Botox Optimizer para revertir.".into());
    }
    let names: Vec<String> = ids.iter().filter_map(|id| cat.tweak(id)).map(|t| format!("• {}", t.name)).collect();
    if !yes {
        return (true, format!("Se revertirían {} cambios:\n{}\n\nPara hacerlo, repetí el comando con --revert-all --yes", ids.len(), names.join("\n")));
    }
    let mut fails = vec![];
    for id in &ids {
        let t = cat.tweak(id).unwrap();
        let d = engine::toggle(&cat, t, false, &Log(None));
        saved.state.insert(id.clone(), d.state);
        if d.ok && !d.state {
            saved.applied.remove(id);
        } else {
            fails.push(format!("• {}: {}", t.name, if d.ok { "Windows todavía no refleja el cambio (puede requerir reinicio)" } else { &d.err }));
        }
    }
    let _ = write_atomic(&saved_path(), serde_json::to_string(&saved).unwrap().as_bytes());
    let ok = ids.len() - fails.len();
    if fails.is_empty() {
        (true, format!("Listo: se revirtieron {ok} cambios. Reiniciá la PC para terminar."))
    } else {
        (false, format!("Se revirtieron {ok} de {}. Fallaron:\n{}", ids.len(), fails.join("\n")))
    }
}

/// Ícono y texto de un aviso en un solo bloque: un layout horizontal acá se estira a toda la altura.
fn toast_text(ui: &mut egui::Ui, icon: &str, t: &Toast) {
    let mut job = egui::text::LayoutJob::default();
    job.append(icon, 0.0, egui::TextFormat { font_id: regular(15.0), color: t.color, ..Default::default() });
    job.append(&t.msg, 9.0, egui::TextFormat { font_id: regular(13.5), color: TEXT, ..Default::default() });
    ui.add(egui::Label::new(job).wrap());
}

fn saved_path() -> std::path::PathBuf {
    crate::win::local_app_data().join("BotoxOptimizer").join("state.json")
}

/// Imagen propia del usuario (reemplaza la del perro), ya recortada a 512×512.
fn avatar_path() -> std::path::PathBuf { saved_path().with_file_name("avatar.png") }

/// Recorta al centro a un cuadrado y lo lleva a 512 px: así cualquier imagen, chica o enorme, vertical u horizontal, llena el marco.
fn square(img: image::DynamicImage) -> image::RgbaImage {
    let (w, h) = (img.width(), img.height());
    let m = w.min(h);
    img.crop_imm((w - m) / 2, (h - m) / 2, m, m).resize_exact(512, 512, image::imageops::FilterType::Lanczos3).to_rgba8()
}

fn avatar_texture(ctx: &egui::Context, img: &image::RgbaImage) -> egui::TextureHandle {
    ctx.load_texture("avatar", egui::ColorImage::from_rgba_unmultiplied([512, 512], img.as_raw()), egui::TextureOptions::LINEAR)
}

/// Abre una imagen mirando su contenido, no la extensión: muchas ".jpg" bajadas de internet son WebP o PNG.
fn open_image(path: impl AsRef<std::path::Path>) -> Result<image::DynamicImage, String> {
    // Límites antes de decodificar: una imagen gigante (o un archivo armado para eso) no se lleva toda la memoria.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 50 << 20) {
        return Err("la imagen pesa más de 50 MB".into());
    }
    let mut r = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string())?;
    let mut lim = image::Limits::default();
    lim.max_image_width = Some(MAX_SIDE);
    lim.max_image_height = Some(MAX_SIDE);
    r.limits(lim);
    r.decode().map_err(|e| e.to_string())
}

/// Lado máximo de una imagen para el logo (luego se achica a 512 px igual).
const MAX_SIDE: u32 = 12_000;

/// Imagen copiada en el portapapeles, o un archivo de imagen copiado en el Explorador.
fn clipboard_image() -> Result<image::DynamicImage, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    if let Ok(i) = cb.get_image() {
        if i.width > MAX_SIDE as usize || i.height > MAX_SIDE as usize {
            return Err("la imagen copiada es demasiado grande".into());
        }
        return image::RgbaImage::from_raw(i.width as u32, i.height as u32, i.bytes.into_owned()).map(image::DynamicImage::ImageRgba8).ok_or_else(|| "imagen dañada".into());
    }
    let file = cb.get().file_list().ok().and_then(|l| l.into_iter().next()).ok_or("no hay nada copiado que sea una imagen.")?;
    open_image(file)
}

/// Escribe a un temporal en la misma carpeta, lo baja a disco y lo renombra encima: un corte de luz deja el archivo
/// viejo o el nuevo, nunca uno a medias.
fn write_atomic(p: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    std::io::Write::write_all(&mut f, data)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, p)
}

fn load_saved() -> Saved { std::fs::read_to_string(saved_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }

pub fn unix_now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        // El logo se usa a ~130 px: se achica una vez con buen filtro para que no se vea dentado.
        let img = image::load_from_memory(include_bytes!("../assets/logo.png")).expect("logo").to_rgba8();
        let img = image::imageops::resize(&img, 264, 264, image::imageops::FilterType::Lanczos3);
        let logo = cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([264, 264], img.as_raw()), egui::TextureOptions::LINEAR);
        let (tx, rx) = channel();
        let (log_tx, log_rx) = channel();
        let saved = load_saved();
        let mut app = Self {
            cat: Arc::new(Catalog::load()),
            page: match win::test_env("BX_PAGE").as_deref() { Some("scan") => Page::Scan, Some("tweaks") => Page::Tweaks, Some("debloat") => Page::Debloat, Some("boost") => Page::Boost, Some("backup") => Page::Backup, Some("startup") => Page::Startup, _ => Page::Home },
            startup: Vec::new(),
            update: None,
            fake: sys::fake_kind(),
            updating: false,
            tab: win::test_env("BX_TAB").unwrap_or_else(|| "all".into()),
            query: String::new(),
            admin: engine::is_admin(),
            spec: None,
            scan_err: None,
            facts: Facts::default(),
            state: HashMap::new(),
            ps_ready: false,
            busy: HashSet::new(),
            reboot_pending: win::test_env("BX_REBOOT").is_some(), // BX_REBOOT=1: muestra el aviso, para capturas
            actions: vec![],
            confirm: None,
            confirm_skip: HashSet::new(),
            queue: VecDeque::new(),
            bulk: None,
            boost_sel: 0,
            boost_out: HashMap::new(),
            restore: None,
            backups: vec![],
            stats: sys::Stats::default(),
            cpu_prev: sys::cpu_times(),
            stats_at: None,
            stats_pending: false,
            saved,
            lines: vec![],
            running: 0,
            run_start: Instant::now(),
            toasts: vec![],
            tx,
            rx,
            log_tx,
            log_rx,
            logo,
            avatar: open_image(avatar_path()).ok().map(|i| avatar_texture(&cc.egui_ctx, &square(i))),
            frames: 0,
            page_at: Instant::now(),
            focus_search: false,
            one_click_medium: false,
            states_at: None,
            verifying: false,
            detail: None,
            rechecking: false,
            rechecked: HashMap::new(),
            had_focus: true,
            safe: std::env::args().any(|a| a == "--safe"),
        };
        if app.safe {
            app.page = Page::Backup;
        }
        // Repara los permisos de los respaldos que dejaron versiones anteriores, y lo dice en la consola.
        if !is_test_copy() && app.admin && reg::backup_dir().exists() {
            let log = Log(Some(app.log_tx.clone()));
            match reg::repair_backups() {
                Ok(()) => log.put("info", "Permisos de la carpeta de respaldos verificados"),
                Err(e) => log.put("err", format!("No se pudieron reparar los permisos de los respaldos: {e}")),
            }
        }
        app.load_all();
        // Arranque instantáneo: muestra lo último que se supo de la PC; los datos frescos llegan en segundos.
        if let Some(spec) = app.saved.spec.clone() {
            app.facts = sys::facts(&spec);
            app.spec = Some(spec);
            app.state = app.saved.state.clone();
        }
        if app.page == Page::Backup {
            app.refresh_backups();
        }
        if app.page == Page::Startup {
            app.startup = startup::scan();
        }
        if app.fake.is_none() {
            app.spawn(|tx, _, _| { let _ = tx.send(Msg::Update(update::check())); });
        }
        app
    }

    /// Lanza un trabajo en otro hilo. Recibe el canal de resultados, el log de la consola y el catálogo.
    fn spawn(&self, job: impl FnOnce(Sender<Msg>, Log, Arc<Catalog>) + Send + 'static) {
        let (tx, log, cat) = (self.tx.clone(), Log(Some(self.log_tx.clone())), self.cat.clone());
        std::thread::spawn(move || job(tx, log, cat));
    }

    fn load_all(&mut self) {
        self.spec = None;
        self.scan_err = None;
        self.ps_ready = false;
        self.spawn(|tx, _, cat| { let _ = tx.send(Msg::Scan(sys::scan(&cat))); });
        self.spawn(|tx, _, cat| { let _ = tx.send(Msg::States { map: engine::check_all_native(&cat), ps: false, na: HashSet::new() }); });
        self.spawn(|tx, _, cat| { let _ = tx.send({ let (map, na) = engine::check_all_ps(&cat); Msg::States { map, ps: true, na } }); });
    }

    fn save(&self) {
        if win::test_env("BX_SHOT").is_some() {
            return; // las capturas de prueba no tocan las preferencias del usuario
        }
        if let Err(e) = write_atomic(&saved_path(), serde_json::to_string(&self.saved).unwrap().as_bytes()) {
            Log(Some(self.log_tx.clone())).put("warn", format!("No se pudieron guardar las preferencias: {e}"));
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>, color: Color32, secs: f32) { self.toasts.push(Toast { msg: msg.into(), color, at: Instant::now(), secs, undo: None }); }
    pub fn on(&self, id: &str) -> bool { self.state.get(id).copied().unwrap_or(false) }
    pub fn boost_last(&self, id: &str) -> Option<u64> { self.saved.boost_last.get(id).copied() }
    pub fn applied(&self) -> &HashSet<String> { &self.saved.applied }
    /// Los tweaks que se muestran: la categoría Notebook solo en notebooks.
    pub fn visible(&self) -> Vec<&catalog::Tweak> { self.cat.tweaks.iter().filter(|t| t.cat != "laptop" || self.facts.laptop).collect() }

    fn need_admin(&mut self) -> bool {
        if !self.admin {
            self.toast("Abrí Botox Optimizer como administrador para aplicar cambios.", RED, 5.0);
        }
        !self.admin
    }

    fn start_toggle(&mut self, id: String, quiet: bool) {
        if self.busy.contains(&id) {
            return;
        }
        let on = !self.on(&id);
        self.busy.insert(id.clone());
        self.spawn(move |tx, log, cat| {
            let t = cat.tweak(&id).unwrap();
            let done = engine::toggle(&cat, t, on, &log);
            let _ = tx.send(Msg::Toggled { id, on, done, quiet });
        });
    }

    fn act(&mut self, mut a: Action) {
        if self.safe {
            if !safe_allows(&a, &|id| self.on(id)) {
                self.toast("Modo seguro: solo se pueden revertir los cambios de Botox Optimizer.", ACCENT, 4.0);
                return;
            }
            // Un lote en modo seguro solo revierte: lo que ya está apagado no se vuelve a aplicar.
            if let Action::ManyNow(ids) = &mut a {
                ids.retain(|id| self.state.get(id) == Some(&true));
            }
        }
        // En modo de prueba solo se puede mirar: ni tweaks, ni limpiezas, ni reinicios, ni actualizaciones, aunque la app tenga permisos de administrador.
        if self.fake.is_some() && !matches!(a, Action::Go(..) | Action::Rescan | Action::RescanStartup | Action::Verify) {
            self.toast("Modo prueba: con hardware simulado no se aplica nada en la PC.", ACCENT, 3.5);
            return;
        }
        match a {
            Action::Go(page, tab) => {
                if page != self.page {
                    self.page_at = Instant::now();
                }
                self.page = page;
                self.tab = tab.unwrap_or("all").into();
                if page == Page::Backup {
                    self.refresh_backups();
                }
                if page == Page::Startup {
                    self.startup = startup::scan();
                }
            }
            Action::RescanStartup => self.startup = startup::scan(),
            Action::Startup(i) => {
                let Some(item) = self.startup.get(i).cloned() else { return };
                if self.need_admin() {
                    return;
                }
                let on = !item.enabled;
                let log = engine::Log(Some(self.log_tx.clone()));
                match startup::set(&item, on) {
                    Ok(()) => {
                        self.startup[i].enabled = on;
                        let msg = format!("{} {} al iniciar Windows", if on { "Encendido:" } else { "Apagado:" }, item.name);
                        log.put("ok", msg.clone());
                        self.toast(msg, if on { GREEN } else { ACCENT }, 3.5);
                    }
                    Err(e) => {
                        log.put("err", format!("{}: {e}", item.name));
                        self.toast(format!("{}: {e}", item.name), RED, 6.0);
                    }
                }
            }
            Action::Toggle(id) => {
                if self.need_admin() {
                    return;
                }
                let t = self.cat.tweak(&id).unwrap();
                if !self.on(&id) && t.risk != "low" {
                    self.confirm = Some(Confirm { text: format!("{}\n\nRiesgo medio: {}\n\n¿Aplicar?", t.name, t.desc), action: Action::ToggleNow(id) });
                } else {
                    self.start_toggle(id, false);
                }
            }
            Action::ToggleNow(id) => self.start_toggle(id, false),
            Action::Many(ids, verb) => {
                if ids.is_empty() || self.need_admin() {
                    return;
                }
                let text = format!("Se van a {verb} {} optimizaciones.\n\nConviene crear antes un punto de restauración (sección Backups).\n\n¿Continuar?", ids.len());
                self.confirm = Some(Confirm { text, action: Action::ManyNow(ids) });
            }
            Action::ManyNow(ids) => {
                self.bulk = Some((ids.len(), 0, false, Vec::new()));
                self.queue = ids.into();
            }
            // Limpiar shaders hace tironear las primeras partidas: se avisa antes.
            Action::RunBoost(id) if id == "shaders" => {
                if !self.need_admin() {
                    let text = "Limpiar caché de shaders

Los juegos van a recompilar sus shaders: las primeras 3 a 5 partidas pueden tironear y después se normaliza. Sirve después de actualizar el driver de video o si un juego se ve con errores gráficos.

¿Continuar?";
                    self.confirm = Some(Confirm { text: text.into(), action: Action::RunBoostNow(id) });
                }
            }
            Action::Reboot => {
                self.confirm = Some(Confirm { text: "Reiniciar la PC

Se va a reiniciar en 5 segundos. Guardá lo que tengas abierto.

¿Continuar?".into(), action: Action::RebootNow });
            }
            Action::Update => {
                if let (Some(u), false) = (&self.update, self.updating) {
                    let text = format!("Actualizar a la versión {}\n\nSe descarga, se verifica el archivo y la app se reinicia sola. Tus ajustes no se tocan.\n\n¿Continuar?", u.version);
                    self.confirm = Some(Confirm { text, action: Action::UpdateNow });
                }
            }
            Action::UpdateNow => {
                let Some(u) = self.update.clone().filter(|_| !self.updating) else { return };
                self.updating = true;
                engine::Log(Some(self.log_tx.clone())).put("start", format!("Actualizando a la versión {}", u.version));
                self.spawn(move |tx, _, _| { let _ = tx.send(Msg::Updated(update::install(&u))); });
            }
            Action::RebootNow => {
                let _ = win::secure_command(win::system32_exe("shutdown.exe")).args(["/r", "/t", "5"]).spawn();
                self.toast("Reiniciando en 5 segundos…", ACCENT, 6.0);
            }
            Action::RunBoost(id) | Action::RunBoostNow(id) => {
                if self.need_admin() || self.busy.contains(&format!("b:{id}")) {
                    return;
                }
                self.busy.insert(format!("b:{id}"));
                self.spawn(move |tx, log, cat| {
                    let b = cat.boosts.iter().find(|b| b.id == id).unwrap();
                    let (ok, out, err) = engine::boost(&cat, b, &log);
                    let _ = tx.send(Msg::Boosted { id, ok, out, err });
                });
            }
            // Un escaneo a la vez: F5 repetido lanzaba escaneos y lotes de PowerShell encima de los que seguían vivos.
            // Mientras hay uno en curso no se lanza otro (Cancelar lo corta y libera).
            Action::Rescan if !self.ps_ready || (self.spec.is_none() && self.scan_err.is_none()) => self.toast("Ya hay un escaneo en curso", ACCENT, 2.5),
            Action::Rescan => self.load_all(),
            // Con una lectura en curso (o el primer escaneo sin terminar) no se lanza otra.
            Action::Verify if self.verifying || !self.ps_ready => {}
            Action::Verify => {
                self.verifying = true;
                self.spawn(|tx, _, cat| { let _ = tx.send(Msg::States { map: engine::check_all_native(&cat), ps: false, na: HashSet::new() }); });
                self.spawn(|tx, _, cat| { let _ = tx.send({ let (map, na) = engine::check_all_ps(&cat); Msg::States { map, ps: true, na } }); });
            }
            Action::Cancel => {
                engine::cancel_all();
                self.toast("Cancelando lo que está en curso…", ACCENT, 4.0);
            }
            Action::CreateRestore => {
                if self.need_admin() || self.busy.contains("rp") {
                    return;
                }
                self.busy.insert("rp".into());
                self.toast("Creando punto de restauración… puede tardar un minuto.", ACCENT, 5.0);
                self.spawn(|tx, log, cat| { let _ = tx.send(Msg::RestoreCreated(engine::create_restore_point(&cat, &log))); });
            }
            Action::OpenRestore => engine::open_system_restore(),
            Action::ImportProfile => {
                if self.need_admin() {
                    return;
                }
                if !self.ps_ready {
                    self.toast("Esperá a que termine de leer el estado de tu PC.", ACCENT, 4.0);
                    return;
                }
                let Some(path) = rfd::FileDialog::new().set_title("Elegí un perfil").add_filter("Perfil de Botox Optimizer", &["botox"]).pick_file() else { return };
                let (p, dropped) = match profile::import(&path, &self.cat) {
                    Ok(x) => x,
                    Err(e) => return self.toast(format!("No se pudo importar: {e}"), RED, 7.0),
                };
                // Solo lo que corresponde a esta PC y se sabe que está apagado; las de riesgo medio, destildadas.
                let f = &self.facts;
                let ids: Vec<String> = p.applied.iter().filter(|id| self.cat.tweak(id).is_some_and(|t| (t.cat != "laptop" || f.laptop) && t.applies(f)) && self.state.get(*id) == Some(&false)).cloned().collect();
                let left = p.applied.len() - ids.len() + dropped;
                if ids.is_empty() {
                    return self.toast("Nada para aplicar: lo del perfil ya está activo o no corresponde a esta PC.", ACCENT, 6.0);
                }
                self.confirm_skip = ids.iter().filter(|id| self.cat.tweak(id).is_some_and(|t| t.risk != "low")).cloned().collect();
                let from = if p.pc_summary.is_empty() { String::new() } else { format!("Perfil de: {}. ", p.pc_summary) };
                let note = if left > 0 { format!("{left} quedan afuera (ya activas, no aplican a esta PC o son de otra versión). ") } else { String::new() };
                let text = format!("Se van a aplicar {} optimizaciones del perfil.\n\n{from}{note}Las de riesgo medio quedan destildadas: tildalas si las querés.\n\n¿Continuar?", ids.len());
                self.confirm = Some(Confirm { text, action: Action::ManyNow(ids) });
            }
            Action::RevertAll => {
                let ids = self.reversible();
                if ids.is_empty() {
                    self.toast("No hay cambios para revertir.", ACCENT, 4.0);
                } else if !self.need_admin() {
                    let text = format!("Se van a revertir {} optimizaciones aplicadas por Botox Optimizer a sus valores originales.\n\n¿Continuar?", ids.len());
                    self.confirm = Some(Confirm { text, action: Action::ManyNow(ids) });
                }
            }
        }
    }

    /// Tweaks activos que la app puede revertir: tienen respaldo o se activaron desde acá.
    pub fn reversible(&self) -> Vec<String> { reversible_ids(&self.cat, &|id| self.on(id), &self.backups, &self.saved.applied) }

    pub fn refresh_backups(&mut self) {
        self.backups = engine::backups();
        self.restore = None;
        self.spawn(|tx, _, cat| { let _ = tx.send(Msg::Restore(engine::restore_points(&cat))); });
    }

    fn handle(&mut self, m: Msg) {
        match m {
            Msg::Scan(Ok(spec)) => {
                let na = std::mem::take(&mut self.facts.na);
                self.facts = sys::facts(&spec);
                self.facts.na = na;
                self.saved.spec = Some(spec.clone());
                self.spec = Some(spec);
                self.save();
            }
            Msg::Scan(Err(e)) => self.scan_err = Some(e),
            Msg::States { map, ps, na } => {
                self.state.extend(map);
                self.ps_ready |= ps;
                if ps {
                    self.facts.na = na;
                    self.states_at = Some(Instant::now());
                    self.verifying = false;
                }
                // También la tanda rápida: si se cierra antes de que llegue la de PowerShell, el próximo arranque
                // muestra igual los estados nativos frescos.
                self.saved.state = self.state.clone();
                self.save();
            }
            Msg::Toggled { id, on, done, quiet } => {
                self.busy.remove(&id);
                self.state.insert(id.clone(), done.state);
                let t = self.cat.tweak(&id).unwrap().clone();
                self.saved.state.insert(id.clone(), done.state);
                if done.ok {
                    if on { self.saved.applied.insert(id.clone()); } else { self.saved.applied.remove(&id); }
                    self.reboot_pending |= t.reboot;
                }
                self.save();
                if let Some(b) = &mut self.bulk {
                    b.1 += done.ok as usize;
                    b.2 |= t.reboot;
                    if done.ok && done.state == on && !done.na {
                        b.3.push(id.clone());
                    }
                }
                if done.na {
                    self.facts.na.insert(id.clone());
                }
                if !done.ok {
                    self.toast(format!("{}: {}", t.name, done.err), RED, 8.0);
                } else if done.na {
                    self.toast(format!("{}: en esta PC no hay nada que cambiar, así que no aplica.", t.name), ACCENT, 6.0);
                } else if done.state != on {
                    self.toast(format!("{}: se ejecutó pero Windows no refleja el cambio todavía (puede requerir reinicio).", t.name), ACCENT, 7.0);
                } else if !quiet {
                    let reboot = if t.reboot && on { " · requiere reiniciar" } else { "" };
                    // Deshacer = el mismo interruptor otra vez (Toggle, no ToggleNow: volver a aplicar uno de riesgo
                    // medio sigue pidiendo confirmación). Solo si quedó como se pidió; si no, iría para el lado equivocado.
                    let msg = format!("{}: {}{reboot}", if on { "Activado" } else { "Revertido" }, t.name);
                    let secs = if t.risk == "low" { 8.0 } else { 12.0 };
                    self.toasts.push(Toast { msg, color: if on { GREEN } else { ACCENT }, at: Instant::now(), secs, undo: Some(Action::Toggle(id.clone())) });
                }
                if self.page == Page::Backup {
                    self.backups = engine::backups();
                }
            }
            Msg::Rechecked(id, st) => {
                self.rechecking = false;
                // Si mientras tanto se aplicó o revirtió, manda el resultado de ese cambio.
                if !self.busy.contains(&id) {
                    self.state.insert(id.clone(), st == Some(true));
                    if st.is_none() { self.facts.na.insert(id); } else { self.facts.na.remove(&id); }
                }
            }
            Msg::Boosted { id, ok, out, err } => {
                self.busy.remove(&format!("b:{id}"));
                let name = self.cat.boosts.iter().find(|b| b.id == id).map(|b| b.name.clone()).unwrap_or_default();
                if ok {
                    self.saved.boost_last.insert(id.clone(), unix_now());
                    self.save();
                    let out = if out.is_empty() { "Listo".to_string() } else { out };
                    self.toast(format!("{name}: {out}"), GREEN, 4.5);
                    self.boost_out.insert(id, out);
                } else {
                    self.toast(format!("{name}: {err}"), RED, 8.0);
                }
            }
            Msg::Update(Ok(Some(u))) => {
                engine::Log(Some(self.log_tx.clone())).put("ok", format!("Hay una versión nueva: {}", u.version));
                self.update = Some(u);
            }
            // Sin internet, sin versiones publicadas o ya al día: no se molesta al usuario.
            Msg::Update(_) => {}
            Msg::Updated(Ok(())) => {
                let log = engine::Log(Some(self.log_tx.clone()));
                log.put("endok", "Versión nueva instalada: reabriendo");
                App::save(self);
                match std::env::current_exe().and_then(|exe| win::secure_command(exe).spawn()) {
                    Ok(_) => std::process::exit(0),
                    Err(e) => {
                        self.updating = false;
                        self.update = None;
                        self.toast(format!("Se actualizó, pero no se pudo reabrir ({e}). Abrila a mano."), ACCENT, 10.0);
                    }
                }
            }
            Msg::Updated(Err(e)) => {
                self.updating = false;
                engine::Log(Some(self.log_tx.clone())).put("enderr", e.clone());
                self.toast(e, RED, 9.0);
            }
            Msg::Restore(r) => self.restore = Some(r),
            Msg::RestoreCreated(r) => {
                self.busy.remove("rp");
                match r {
                    Ok(()) => self.toast("Punto de restauración creado.", GREEN, 5.0),
                    Err(e) => self.toast(format!("No se pudo crear: {e} (Windows permite uno cada 24 h y requiere la protección del sistema activa)."), RED, 9.0),
                }
                self.refresh_backups();
            }
            Msg::Stats(s, prev) => {
                self.stats = s;
                self.cpu_prev = prev;
                self.stats_pending = false;
            }
        }
    }

    /// Procesa lo que mandaron los hilos, avanza la cola de cambios en lote y pide el uso en vivo.
    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m);
        }
        while let Ok(l) = self.log_rx.try_recv() {
            match l.lvl.as_str() {
                "start" => {
                    if self.running == 0 {
                        self.run_start = Instant::now();
                    }
                    self.running += 1;
                }
                "enderr" => {
                    self.running = self.running.saturating_sub(1);
                    self.saved.console_open = true; // si algo falla, la consola se abre sola
                }
                x if x.starts_with("end") => self.running = self.running.saturating_sub(1),
                _ => {}
            }
            self.lines.push(l);
            if self.lines.len() > 2000 {
                self.lines.remove(0);
            }
        }
        // Cambios en lote: de a uno, para que los respaldos y la consola queden en orden.
        let toggling = self.busy.iter().any(|b| !b.starts_with("b:") && b != "rp");
        if !toggling {
            if let Some(id) = self.queue.pop_front() {
                self.start_toggle(id, true);
            } else if let Some((total, ok, reboot, changed)) = self.bulk.take() {
                let tail = if reboot { " Reiniciá la PC para terminar de aplicar." } else { "" };
                // Un solo "Deshacer" para todo el lote: vuelve atrás, en la misma cola, solo lo que quedó como se pidió.
                let undo = (!changed.is_empty()).then(|| Action::ManyNow(changed));
                self.toasts.push(Toast { msg: format!("{ok}/{total} listas.{tail}"), color: if ok == total { GREEN } else { ACCENT }, at: Instant::now(), secs: 12.0, undo });
            }
        }
        // Uso en vivo: solo en Inicio y con el mouse sobre la ventana, para no gastar nada de fondo.
        let watching = self.page == Page::Home && ctx.input(|i| i.pointer.has_pointer());
        if watching && !self.stats_pending && self.stats_at.map_or(true, |t| t.elapsed() > Duration::from_secs(2)) {
            self.stats_pending = true;
            self.stats_at = Some(Instant::now());
            let prev = self.cpu_prev;
            self.spawn(move |tx, _, _| { let (s, now) = sys::stats(prev); let _ = tx.send(Msg::Stats(s, now)); });
        }
        let loading = self.spec.is_none() && self.scan_err.is_none() || !self.ps_ready || self.restore.is_none() && self.page == Page::Backup;
        if !self.busy.is_empty() || self.running > 0 || loading || self.verifying || self.stats_pending || !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if watching {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }

    /// Dibuja el logo flotando (sube y baja). Solo se anima con el mouse sobre la ventana, para no gastar GPU de fondo.
    pub fn floating_logo(&self, ui: &egui::Ui, center: Pos2, size: f32) {
        let dy = if ui.input(|i| i.pointer.has_pointer()) {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
            (ui.input(|i| i.time) * 1.85).sin() as f32 * size * 0.03
        } else {
            0.0
        };
        let rect = Rect::from_center_size(center + Vec2::new(0.0, dy), Vec2::splat(size));
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let Some(tex) = &self.avatar else {
            ui.painter().image(self.logo.id(), rect, uv, Color32::WHITE);
            return;
        };
        // Imagen propia con el mismo marco que el logo: medidas tomadas de logo.png (512 px).
        let (p, s) = (ui.painter(), size);
        let bx = Rect::from_center_size(rect.center() - Vec2::new(0.0, 0.07 * s), Vec2::splat(0.707 * s));
        let r = 0.137 * s;
        let px = |f: f32| (f * s).round().clamp(1.0, 127.0);
        p.add(egui::epaint::Shadow { offset: [0, px(0.09) as i8], blur: px(0.12) as u8, spread: 0, color: Color32::from_black_alpha(150) }.as_shape(bx, r));
        p.add(egui::epaint::Shadow { offset: [0, 0], blur: px(0.08) as u8, spread: 0, color: ACCENT.gamma_multiply(0.35) }.as_shape(bx, r));
        p.rect_filled(bx, r, CARD2); // fondo para imágenes con transparencia
        p.add(egui::epaint::RectShape::filled(bx, r, Color32::WHITE).with_texture(tex.id(), uv));
        p.rect_stroke(bx, r, egui::Stroke::new((0.008 * s).max(1.0), ACCENT), egui::StrokeKind::Inside);
        let bc = rect.center() + Vec2::new(0.262 * s, 0.199 * s);
        p.circle_filled(bc, 0.133 * s, Color32::from_rgb(14, 18, 32));
        p.circle_filled(bc, 0.112 * s, ACCENT);
        p.text(bc, Align2::CENTER_CENTER, ic::LIGHTNING, regular(0.14 * s), Color32::WHITE);
    }

    /// Cambia la imagen propia (o la quita con None) y la guarda para la próxima vez.
    fn set_avatar(&mut self, ctx: &egui::Context, img: Option<Result<image::DynamicImage, String>>) {
        match img {
            None => {
                let _ = std::fs::remove_file(avatar_path());
                self.avatar = None;
                self.toast("Volviste a la imagen original.", GREEN, 3.0);
            }
            Some(Ok(i)) => {
                let sq = square(i);
                let _ = std::fs::create_dir_all(avatar_path().parent().unwrap());
                if let Err(e) = sq.save(avatar_path()) {
                    self.toast(format!("La imagen se ve, pero no se pudo guardar: {e}"), YELLOW, 5.0);
                }
                self.avatar = Some(avatar_texture(ctx, &sq));
                self.toast("Imagen cambiada.", GREEN, 3.0);
            }
            Some(Err(e)) => self.toast(format!("No se pudo usar la imagen: {e}"), RED, 5.0),
        }
    }

    /// Barra lateral: marca arriba, secciones con nombre y estado de permisos abajo.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let (brand, _) = ui.allocate_exact_size(Vec2::new(full.width(), 92.0), Sense::hover());
        let c = brand.left_center() + Vec2::new(42.0, 2.0);
        // Clic en el logo: menú para poner una imagen propia.
        let hit = ui.interact(Rect::from_center_size(c, Vec2::splat(54.0)), ui.id().with("logo"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if hit.hovered() {
            ui.painter().rect_filled(hit.rect.expand(4.0), 12.0, Color32::from_white_alpha(10));
        }
        self.floating_logo(ui, c, 54.0);
        let menu = ui.id().with("logo-menu");
        if hit.clicked() {
            ui.memory_mut(|m| m.toggle_popup(menu));
        }
        let hit = if ui.memory(|m| m.is_popup_open(menu)) { hit } else { hit.on_hover_text("Cambiar imagen") };
        let mut pick = None;
        egui::popup::popup_below_widget(ui, menu, &hit, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(230.0);
            let item = |ui: &mut egui::Ui, icon: &str, t: &str| ui.add(egui::Button::new(RichText::new(format!("{icon}   {t}")).size(14.0)).frame(false).min_size(Vec2::new(222.0, 32.0))).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            if item(ui, ic::FOLDER_OPEN, "Elegir imagen…") {
                pick = Some(1);
            }
            if item(ui, ic::CLIPBOARD_TEXT, "Pegar imagen copiada") {
                pick = Some(2);
            }
            if self.avatar.is_some() && item(ui, ic::ARROW_COUNTER_CLOCKWISE, "Volver a la original") {
                pick = Some(3);
            }
        });
        let ctx = ui.ctx().clone();
        match pick {
            Some(1) => {
                let file = rfd::FileDialog::new().set_title("Elegí una imagen").add_filter("Imágenes", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff"]).pick_file();
                if let Some(f) = file {
                    self.set_avatar(&ctx, Some(open_image(f)));
                }
            }
            Some(2) => self.set_avatar(&ctx, Some(clipboard_image())),
            Some(3) => self.set_avatar(&ctx, None),
            _ => {}
        }
        if self.running > 0 {
            // Punto naranja: hay algo corriendo, aunque estés en otra sección.
            let a = (ui.input(|i| i.time) * 5.0).sin() as f32 * 0.35 + 0.65;
            ui.painter().circle_filled(c + Vec2::new(20.0, -20.0), 5.5, ACCENT.gamma_multiply(a));
        }
        ui.painter().text(brand.left_center() + Vec2::new(76.0, -8.0), Align2::LEFT_CENTER, "Botox", bold(19.0), TEXT);
        ui.painter().text(brand.left_center() + Vec2::new(77.0, 13.0), Align2::LEFT_CENTER, "OPTIMIZER", semi(10.5), ACCENT);
        ui.add_space(6.0);
        for (i, (page, icon, name)) in NAV.iter().enumerate() {
            // Modo seguro: solo Backups.
            if self.safe && *page != Page::Backup {
                continue;
            }
            if i == 6 && !self.safe {
                let (r, _) = ui.allocate_exact_size(Vec2::new(full.width(), 10.0), Sense::hover());
                ui.painter().hline(r.x_range().shrink(22.0), r.center().y, egui::Stroke::new(1.0_f32, LINE));
            }
            let (slot, resp) = ui.allocate_exact_size(Vec2::new(full.width(), 44.0), Sense::click());
            let rect = slot.shrink2(Vec2::new(12.0, 0.0));
            let (on, hov) = (self.page == *page, resp.hovered());
            let h = ui.ctx().animate_bool_with_time(ui.id().with(("nav", i)), on || hov, 0.12);
            let p = ui.painter().clone();
            if h > 0.0 {
                p.rect_filled(rect, 10.0, mix(SIDE, if on { Color32::from_rgb(30, 39, 70) } else { Color32::from_rgb(23, 31, 58) }, h));
            }
            if on {
                p.rect_filled(Rect::from_min_size(Pos2::new(slot.left(), rect.top() + 11.0), Vec2::new(3.0, 22.0)), 2.0, ACCENT);
            }
            let col = if on { ACCENT } else if hov { TEXT } else { MUTED };
            p.text(rect.left_center() + Vec2::new(16.0, 0.0), Align2::LEFT_CENTER, *icon, regular(20.0), col);
            p.text(rect.left_center() + Vec2::new(48.0, 1.0), Align2::LEFT_CENTER, *name, if on { semi(14.0) } else { medium(14.0) }, if on { TEXT } else { col });
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("Ctrl+{}", i + 1)).clicked() {
                self.actions.push(Action::Go(*page, None));
            }
        }
        // Pie: si tiene permisos de administrador y la versión.
        let p = ui.painter();
        let pill = Rect::from_min_size(Pos2::new(full.left() + 14.0, full.bottom() - 70.0), Vec2::new(full.width() - 28.0, 36.0));
        p.rect(pill, 10.0, CARD, egui::Stroke::new(1.0_f32, LINE), egui::StrokeKind::Inside);
        let (dot, text) = if self.admin { (GREEN, "Administrador") } else { (YELLOW, "Solo lectura") };
        p.circle_filled(pill.left_center() + Vec2::new(16.0, 0.0), 4.5, dot);
        p.text(pill.left_center() + Vec2::new(30.0, 1.0), Align2::LEFT_CENTER, text, medium(13.0), TEXT);
        p.text(Pos2::new(pill.center().x, full.bottom() - 18.0), Align2::CENTER_CENTER, format!("v{}", env!("CARGO_PKG_VERSION")), regular(11.5), DIM);
    }

    /// Atajos de teclado: Ctrl+1…7 secciones, Ctrl+F buscar, Ctrl+J consola, F5 escanear, Esc limpia la búsqueda.
    fn keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        if self.confirm.is_some() {
            return;
        }
        let nums = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7];
        for (i, k) in nums.into_iter().enumerate() {
            if ctx.input_mut(|inp| inp.consume_key(Modifiers::CTRL, k)) {
                self.actions.push(Action::Go(NAV[i].0, None));
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::F)) {
            if !matches!(self.page, Page::Tweaks | Page::Debloat) {
                self.actions.push(Action::Go(Page::Tweaks, None));
            }
            self.focus_search = true;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::J)) {
            self.saved.console_open = !self.saved.console_open;
            self.save();
        }
        // Ctrl+Shift+R: "Revertir todo" desde cualquier sección (con la confirmación de siempre).
        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::R)) {
            self.actions.push(Action::RevertAll);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5)) {
            self.actions.push(Action::Rescan);
        }
        // Al volver a la ventana después de un rato, el estado se relee de fondo: pudo cambiar fuera de la app.
        let focus = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focus && !self.had_focus && self.states_at.is_some_and(|t| t.elapsed() > Duration::from_secs(15 * 60)) {
            self.actions.push(Action::Verify);
        }
        self.had_focus = focus;
        if !self.query.is_empty() && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.query.clear();
        }
    }

    fn console(&mut self, ui: &mut egui::Ui) {
        let color = |lvl: &str| match lvl {
            "start" => ACCENT,
            "ok" | "endok" => GREEN,
            "err" | "enderr" => RED,
            "warn" | "endwarn" => YELLOW,
            "out" | "cmd" => MUTED,
            _ => Color32::from_rgb(200, 208, 226),
        };
        // En pantalla van íconos; al copiar, texto plano que se lee en cualquier lado.
        let prefix = |lvl: &str| match lvl { "start" => format!("{} ", ic::PLAY), "endok" | "ok" => format!("{} ", ic::CHECK), "enderr" | "err" => format!("{} ", ic::X), "endwarn" | "warn" => format!("{} ", ic::WARNING), _ => "  ".to_string() };
        let plain = |lvl: &str| match lvl { "start" => "> ", "endok" | "ok" => "OK ", "enderr" | "err" => "ERROR ", "endwarn" | "warn" => "AVISO ", _ => "   " };
        // La barra entera abre y cierra la consola. Se registra ANTES que los botones para quedar debajo:
        // registrada después, se comía los clics de "Copiar" y "Limpiar".
        let bar_rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.max_rect().width(), 38.0));
        let bar = ui.interact(bar_rect, ui.id().with("console-bar"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.horizontal(|ui| {
            ui.set_height(38.0);
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            let a = if self.running > 0 { (ui.input(|i| i.time) * 5.0).sin() as f32 * 0.35 + 0.65 } else { 1.0 };
            ui.painter().circle_filled(dot.center(), 4.5, if self.running > 0 { ACCENT.gamma_multiply(a) } else { DIM });
            ui.label(RichText::new("Consola").font(semi(12.5)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let small = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(RichText::new(text).font(medium(12.0)).color(MUTED)).fill(CARD2).stroke(egui::Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 24.0))).on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.spacing_mut().button_padding = Vec2::new(11.0, 3.0);
                if small(ui, if self.saved.console_open { ic::CARET_DOWN } else { ic::CARET_UP }).on_hover_text("Mostrar u ocultar (Ctrl+J)").clicked() {
                    self.saved.console_open = !self.saved.console_open;
                    self.save();
                }
                if small(ui, "Limpiar").clicked() {
                    self.lines.clear();
                }
                if small(ui, "Copiar diagnóstico").on_hover_text("Copia el registro con la versión, Windows y el hardware, listo para pegar en Discord o GitHub. Sin tu usuario ni el nombre de la PC. No se envía nada: solo va al portapapeles.").clicked() {
                    let lines: Vec<String> = self.lines.iter().map(|l| format!("{} {}{}", l.t, plain(&l.lvl), l.msg)).collect();
                    let report = diag::report(self.spec.as_ref(), self.admin, self.fake.as_deref(), &lines, &win::user_profile().display().to_string(), &std::env::var("COMPUTERNAME").unwrap_or_default());
                    ui.ctx().copy_text(report);
                    self.toast("Diagnóstico copiado al portapapeles", GREEN, 3.0);
                }
                if self.running > 0 && small(ui, "Cancelar").on_hover_text("Detener lo que está en curso").clicked() {
                    self.actions.push(Action::Cancel);
                }
                if self.running > 0 {
                    let s = self.run_start.elapsed().as_secs();
                    ui.label(RichText::new(format!("{} {}:{:02}", ic::TIMER, s / 60, s % 60)).font(mono(12.5)).color(ACCENT));
                }
                // Última línea (sin la salida cruda), cortada si no entra.
                let last = self.lines.iter().rev().find(|l| l.lvl != "out");
                let (text, c) = last.map_or(("Acá vas a ver en vivo todo lo que hace la app.".to_string(), MUTED), |l| (format!("{}{}", prefix(&l.lvl).trim_start(), l.msg), color(&l.lvl)));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(text).font(mono(12.5)).color(c)).truncate());
                });
            });
        });
        if bar.clicked() {
            self.saved.console_open = !self.saved.console_open;
            self.save();
        }
        if self.saved.console_open {
            egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.style_mut().interaction.selectable_labels = true;
                for l in &self.lines {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&l.t).font(mono(12.5)).color(DIM));
                        let text = RichText::new(format!("{}{}", prefix(&l.lvl), l.msg)).font(mono(12.5)).color(color(&l.lvl));
                        ui.add(egui::Label::new(if l.lvl == "start" { text.strong() } else { text }).wrap());
                    });
                }
                ui.add_space(8.0);
            });
        }
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        // Avisos abajo a la derecha.
        self.toasts.retain(|t| t.at.elapsed().as_secs_f32() < t.secs);
        let mut bottom = if self.saved.console_open { 300.0 } else { 52.0 };
        let mut undone = None;
        // Un Area por aviso, apilados de abajo hacia arriba. Un Area, aunque no sea interactivo, tapa los clics de todo
        // lo que queda debajo de su rectángulo: con uno solo en bottom_up ocupaba la columna derecha entera. Solo los
        // que traen "Deshacer" reciben clics, y solo en su propio rectángulo.
        let n = self.toasts.len();
        for (i, t) in self.toasts.iter().rev().enumerate() {
            let shown = egui::Area::new(egui::Id::new(("toast", i))).anchor(egui::Align2::RIGHT_BOTTOM, [-22.0, -bottom]).interactable(t.undo.is_some()).show(ctx, |ui| {
                // Aparece y se va con un fundido corto.
                let age = t.at.elapsed().as_secs_f32();
                let alpha = (age / 0.15).min(1.0).min((t.secs - age) / 0.4).clamp(0.0, 1.0);
                let icon = if t.color == GREEN { ic::CHECK_CIRCLE } else if t.color == RED { ic::WARNING_CIRCLE } else { ic::INFO };
                ui.multiply_opacity(alpha);
                let r = egui::Frame::new().fill(Color32::from_rgb(27, 36, 67)).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(10).inner_margin(egui::Margin { left: 18, right: 16, top: 12, bottom: 12 }).show(ui, |ui| {
                    ui.set_max_width(if t.undo.is_some() { 500.0 } else { 420.0 });
                    ui.horizontal(|ui| {
                        if t.undo.is_some() {
                            // El botón primero (a la derecha) y el texto en lo que sobra.
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().button_padding = Vec2::new(12.0, 4.0);
                                let b = egui::Button::new(RichText::new("Deshacer").font(semi(13.0)).color(ACCENT)).fill(CARD2).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(8);
                                if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    undone = Some(n - 1 - i);
                                }
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| toast_text(ui, icon, t));
                            });
                        } else {
                            toast_text(ui, icon, t);
                        }
                    });
                });
                let bar = Rect::from_min_size(r.response.rect.min, Vec2::new(4.0, r.response.rect.height()));
                ui.painter().rect_filled(bar, egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, t.color);
            });
            bottom += shown.response.rect.height() + 8.0;
        }
        if let Some(i) = undone {
            if let Some(a) = self.toasts.remove(i).undo {
                self.actions.push(a);
            }
        }
        // "Qué cambia" de un tweak (#9): solo lo que dice el catálogo, sin textos ni fuentes inventados.
        if let Some(t) = self.detail.as_ref().and_then(|id| self.cat.tweak(id)).cloned() {
            let mut close = false;
            egui::Modal::new(egui::Id::new("detail")).frame(panel()).show(ctx, |ui| {
                ui.set_width(620.0);
                ui.add(egui::Label::new(RichText::new(&t.name).font(semi(17.0))).wrap());
                ui.add(egui::Label::new(RichText::new(&t.desc).size(14.0).color(Color32::from_rgb(200, 208, 226))).wrap());
                let mut facts = vec![self.cat.cat_name(&t.cat).to_string(), if t.risk == "low" { "Riesgo bajo".into() } else { "Riesgo medio".into() }];
                if t.reboot {
                    facts.push("Necesita reiniciar".into());
                }
                ui.label(RichText::new(facts.join(" · ")).size(12.5).color(MUTED));
                ui.add_space(4.0);
                ui.label(RichText::new("Qué cambia").font(semi(14.0)));
                let changes = t.changes();
                for c in &changes {
                    ui.add(egui::Label::new(RichText::new(format!("• {c}")).font(mono(12.5))).wrap());
                }
                if let Some(ps) = &t.ps {
                    egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(10).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&ps.apply).font(mono(11.5)).color(MUTED)).wrap());
                        });
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if button(ui, "Copiar detalle", Btn::Primary).clicked() {
                        let script = t.ps.as_ref().map_or(String::new(), |ps| format!("\n\nScript:\n{}", ps.apply));
                        ui.ctx().copy_text(format!("{}\n{}\n\nQué cambia:\n- {}{script}", t.name, t.desc, changes.join("\n- ")));
                        self.toasts.push(Toast { msg: "Detalle copiado.".into(), color: GREEN, at: Instant::now(), secs: 3.0, undo: None });
                    }
                    if button(ui, "Cerrar", Btn::Ghost).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
            if close {
                self.detail = None;
            }
        }
        // Confirmación.
        let mut answer = None;
        let mut selected = usize::MAX; // en lote: cuántas quedaron tildadas
        if let Some(c) = &self.confirm {
            egui::Modal::new(egui::Id::new("confirm")).frame(panel()).show(ctx, |ui| {
                ui.set_width(520.0);
                // La primera línea es el título; el resto, el detalle.
                let (head, body) = c.text.split_once("\n\n").unwrap_or((&c.text, ""));
                // En lote, la pregunta va debajo de la lista.
                let many = matches!(c.action, Action::ManyNow(_));
                let body = if many { body.trim_end_matches("¿Continuar?").trim_end() } else { body };
                // El título cuenta solo las que siguen tildadas.
                let head = match &c.action {
                    Action::ManyNow(ids) => head.replacen(&ids.len().to_string(), &ids.iter().filter(|id| !self.confirm_skip.contains(*id)).count().to_string(), 1),
                    _ => head.to_string(),
                };
                ui.add(egui::Label::new(RichText::new(head).font(semi(17.0))).wrap());
                if !body.is_empty() {
                    ui.add(egui::Label::new(RichText::new(body).size(14.0).color(Color32::from_rgb(200, 208, 226))).wrap());
                }
                // En lote: la lista de lo que se va a tocar, para revisarla antes de aceptar.
                if let Action::ManyNow(ids) = &c.action {
                    let skip = &mut self.confirm_skip;
                    let list: Vec<&catalog::Tweak> = ids.iter().filter_map(|id| self.cat.tweak(id)).collect();
                    let picked: Vec<&&catalog::Tweak> = list.iter().filter(|t| !skip.contains(&t.id)).collect();
                    let reboot = picked.iter().filter(|t| t.reboot).count();
                    selected = picked.len();
                    ui.horizontal(|ui| {
                        let of = if selected < list.len() { format!(" de {}", list.len()) } else { String::new() };
                        let rb = if reboot > 0 { format!(" · {reboot} necesitan reiniciar") } else { String::new() };
                        ui.label(RichText::new(format!("Incluye: {selected}{of}{rb}")).font(semi(13.0)).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Atajos para marcar o desmarcar todo.
                            let link = |ui: &mut egui::Ui, t: &str| ui.add(egui::Label::new(RichText::new(t).size(12.5).color(ACCENT2)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
                            if link(ui, "Ninguna") {
                                skip.extend(list.iter().map(|t| t.id.clone()));
                            }
                            if link(ui, "Todas") {
                                skip.clear();
                            }
                        });
                    });
                    egui::Frame::new().fill(CARD).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            // Tilde naranja, para que se distinga de un vistazo.
                            let wv = &mut ui.visuals_mut().widgets;
                            for w in [&mut wv.inactive, &mut wv.hovered, &mut wv.active] {
                                w.fg_stroke = egui::Stroke::new(2.0_f32, ACCENT);
                            }
                            for t in list {
                                ui.horizontal(|ui| {
                                    // Destildar = dejarla afuera de este lote.
                                    let mut on = !skip.contains(&t.id);
                                    if ui.checkbox(&mut on, "").on_hover_cursor(egui::CursorIcon::PointingHand).changed() {
                                        if on { skip.remove(&t.id); } else { skip.insert(t.id.clone()); }
                                    }
                                    ui.add(egui::Label::new(RichText::new(&t.name).size(13.5).color(if on { TEXT } else { DIM })).truncate());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if t.reboot {
                                            ui.label(RichText::new(ic::ARROWS_CLOCKWISE).size(13.0).color(MUTED)).on_hover_text("Necesita reiniciar");
                                        }
                                        ui.label(RichText::new(self.cat.cat_name(&t.cat)).size(12.0).color(DIM));
                                    });
                                });
                            }
                        });
                    });
                    ui.label(RichText::new("¿Continuar?").size(14.0));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if selected == usize::MAX { "Continuar".to_string() } else { format!("Continuar con {selected}") };
                    if ui.add_enabled_ui(selected > 0, |ui| button(ui, &label, Btn::Primary)).inner.clicked() {
                        answer = Some(true);
                    }
                    if button(ui, "Cancelar", Btn::Ghost).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        answer = Some(false);
                    }
                });
            });
        }
        if let Some(yes) = answer {
            let mut c = self.confirm.take().unwrap();
            let skip = std::mem::take(&mut self.confirm_skip);
            if let Action::ManyNow(ids) = &mut c.action {
                ids.retain(|id| !skip.contains(id));
            }
            if yes {
                self.actions.push(c.action);
            }
        }
    }

    /// Captura para pruebas: BX_SHOT=archivo.png guarda la ventana cuando terminó de cargar y cierra.
    /// BX_PAGE / BX_TAB eligen la pantalla; BX_DEMO=1 corre "Vaciar caché DNS" para ver la consola.
    fn shot(&mut self, ctx: &egui::Context) {
        let Some(path) = win::test_env("BX_SHOT") else { return };
        ctx.request_repaint();
        let ready = self.spec.is_some() && self.ps_ready && (self.page != Page::Backup || self.restore.is_some());
        if ready && self.frames == 0 && win::test_env("BX_DEMO").is_some() {
            self.admin = true;
            self.saved.console_open = true;
            self.actions.push(Action::RunBoost("dns".into()));
            self.frames = 1;
        }
        // BX_CONFIRM=1: muestra la confirmación de "Activar … de bajo riesgo" con todos los tweaks (no aplica nada).
        if ready && self.frames == 0 && self.confirm.is_none() && win::test_env("BX_CONFIRM").is_some() {
            let ids = self.cat.tweaks.iter().filter(|t| t.risk == "low").map(|t| t.id.clone()).collect::<Vec<_>>();
            self.confirm_skip.extend(ids.iter().take(2).cloned()); // para ver casillas destildadas
            self.confirm = Some(Confirm { text: format!("Se van a activar {} optimizaciones.\n\nConviene crear antes un punto de restauración (sección Backups).\n\n¿Continuar?", ids.len()), action: Action::ManyNow(ids) });
        }
        // BX_UNDO=1: muestra el aviso de un cambio con su botón "Deshacer" (no aplica nada).
        if ready && self.frames == 0 && self.toasts.is_empty() && win::test_env("BX_UNDO").is_some() {
            let id = self.cat.tweaks[0].id.clone();
            let msg = format!("Activado: {}", self.cat.tweaks[0].name);
            self.toasts.push(Toast { msg, color: GREEN, at: Instant::now(), secs: 60.0, undo: Some(Action::Toggle(id)) });
        }
        // BX_DETAIL=id: abre el panel "Qué cambia" de ese tweak.
        if ready && self.frames == 0 && self.detail.is_none() {
            self.detail = win::test_env("BX_DETAIL");
        }
        if ready && self.busy.is_empty() {
            self.frames += 1;
        }
        if self.frames == 40 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let img = ctx.input(|i| i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = img {
            let _ = image::save_buffer(&path, img.as_raw(), img.width() as u32, img.height() as u32, image::ColorType::Rgba8);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        self.shot(ctx);
        self.keys(ctx);
        // Recuerda la ventana (se guarda al cerrar).
        let vp = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().maximized, i.viewport().minimized));
        if let (Some(o), Some(inn), Some(max), Some(false)) = vp {
            self.saved.maximized = max;
            if !max {
                self.saved.win = Some([o.min.x, o.min.y, inn.width(), inn.height()]);
            }
        }
        egui::SidePanel::left("nav").exact_width(SIDE_W).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(SIDE)).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
            self.sidebar(ui);
        });
        egui::TopBottomPanel::bottom("console")
            .exact_height(if self.saved.console_open { 288.0 } else { 38.0 })
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(CONSOLE).inner_margin(egui::Margin::symmetric(18, 0)))
            .show(ctx, |ui| self.console(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 40, right: 28, top: 28, bottom: 0 })).show(ctx, |ui| {
            // Fundido y leve subida al cambiar de sección.
            let t = (self.page_at.elapsed().as_secs_f32() / 0.22).min(1.0);
            if t < 1.0 {
                ctx.request_repaint();
            }
            let e = 1.0 - (1.0 - t).powi(3);
            ui.multiply_opacity(e);
            egui::ScrollArea::vertical().auto_shrink(false).id_salt(self.page as u8).show(ui, |ui| {
                ui.set_width(ui.available_width() - 12.0);
                ui.add_space((1.0 - e) * 14.0);
                self.page(ui);
                ui.add_space(30.0);
            });
        });
        self.overlays(ctx);
        for a in std::mem::take(&mut self.actions) {
            self.act(a);
        }
        // Primer cuadro dibujado: si veníamos de una actualización, quedó bien y se borra la copia vieja.
        static FIRST_FRAME: std::sync::Once = std::sync::Once::new();
        FIRST_FRAME.call_once(|| {
            if !is_test_copy() {
                update::confirm_ok()
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) { App::save(self); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_hover_rechequea_una_sola_vez() {
        let (min, s) = (|m: u64| Some(Duration::from_secs(m * 60)), Duration::from_secs);
        // Estado viejo, nunca re-chequeado y nada en curso: sí.
        assert!(should_recheck(min(6), None, false));
        // Estado fresco o todavía sin leer: no.
        assert!(!should_recheck(min(2), None, false) && !should_recheck(None, None, false));
        // Ya hay uno en curso (pasar el mouse por toda la lista): no se abre otro.
        assert!(!should_recheck(min(6), None, true));
        // El mismo interruptor hace menos de 5 min: no; más de 5 min: sí.
        assert!(!should_recheck(min(6), Some(s(30)), false));
        assert!(should_recheck(min(20), min(6), false));
    }

    #[test]
    fn modo_seguro_solo_deja_revertir() {
        let on = |id: &str| id == "gamemode";
        for a in [Action::RevertAll, Action::CreateRestore, Action::OpenRestore, Action::Go(Page::Backup, None), Action::Toggle("gamemode".into()), Action::Reboot, Action::Verify] {
            assert!(safe_allows(&a, &on));
        }
        // Encender algo apagado, aplicar en lote, limpiar, actualizar o ir a otra sección: no.
        for a in [Action::Toggle("hags".into()), Action::ToggleNow("hags".into()), Action::Many(vec!["hags".into()], "aplicar"), Action::RunBoost("temp".into()), Action::UpdateNow, Action::Go(Page::Tweaks, None), Action::Startup(0)] {
            assert!(!safe_allows(&a, &on));
        }
    }

    #[test]
    fn revertible_es_lo_activo_con_respaldo_o_aplicado_desde_aca() {
        let cat = Catalog::load();
        let t = std::time::SystemTime::now();
        let backups = vec![("gamemode".to_string(), t), ("niclatency-nic".to_string(), t), ("no-existe".to_string(), t)];
        let applied: HashSet<String> = ["hags".to_string(), "gamedvr".to_string()].into();
        let ids = reversible_ids(&cat, &|id| id != "gamedvr", &backups, &applied);
        // gamedvr está apagado; un id que no es del catálogo se ignora; "-nic" se une con su tweak.
        let mut expected: Vec<String> = ["gamemode", "hags"].iter().map(|s| s.to_string()).collect();
        if cat.tweak("niclatency").is_some() {
            expected.push("niclatency".into());
        }
        expected.sort();
        assert_eq!(ids, expected);
    }
}
