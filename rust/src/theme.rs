//! Colores, fuentes y widgets propios (interruptor, anillo, tarjeta, botones).
use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Response, RichText, Sense, Shape, Stroke, Ui, Vec2,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(11, 16, 32);
pub const SIDE: Color32 = Color32::from_rgb(14, 20, 40);
pub const PANEL: Color32 = Color32::from_rgb(18, 26, 48);
pub const CARD: Color32 = Color32::from_rgb(22, 31, 56);
pub const CARD2: Color32 = Color32::from_rgb(27, 38, 67);
pub const LINE: Color32 = Color32::from_rgb(34, 45, 74);
pub const TEXT: Color32 = Color32::from_rgb(232, 236, 245);
pub const MUTED: Color32 = Color32::from_rgb(139, 149, 173);
pub const DIM: Color32 = Color32::from_rgb(93, 103, 130);
pub const ACCENT: Color32 = Color32::from_rgb(247, 147, 30);
pub const ACCENT2: Color32 = Color32::from_rgb(255, 179, 71);
pub const BLUE: Color32 = Color32::from_rgb(61, 108, 245);
pub const GREEN: Color32 = Color32::from_rgb(47, 209, 122);
pub const RED: Color32 = Color32::from_rgb(255, 90, 95);
pub const YELLOW: Color32 = Color32::from_rgb(255, 194, 61);
pub const CONSOLE: Color32 = Color32::from_rgb(7, 10, 21);
pub const TRACK: Color32 = Color32::from_rgb(29, 39, 66);

pub fn regular(size: f32) -> FontId { FontId::new(size, FontFamily::Proportional) }
pub fn medium(size: f32) -> FontId { FontId::new(size, FontFamily::Name("medium".into())) }
pub fn semi(size: f32) -> FontId { FontId::new(size, FontFamily::Name("semibold".into())) }
pub fn bold(size: f32) -> FontId { FontId::new(size, FontFamily::Name("bold".into())) }
pub fn mono(size: f32) -> FontId { FontId::new(size, FontFamily::Monospace) }

pub fn install(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut f, egui_phosphor::Variant::Regular);
    // Cada peso de Poppins es una familia; todas caen en los íconos y fuentes por defecto si falta un glifo.
    let fallback = f.families[&FontFamily::Proportional].clone();
    let faces: [(&str, &'static [u8]); 4] = [
        ("regular", include_bytes!("../assets/Poppins-Regular.ttf")),
        ("medium", include_bytes!("../assets/Poppins-Medium.ttf")),
        ("semibold", include_bytes!("../assets/Poppins-SemiBold.ttf")),
        ("bold", include_bytes!("../assets/Poppins-Bold.ttf")),
    ];
    for (name, bytes) in faces {
        f.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
        let mut list = vec![name.to_owned()];
        list.extend(fallback.clone());
        let family = if name == "regular" { FontFamily::Proportional } else { FontFamily::Name(name.into()) };
        f.families.insert(family, list);
    }
    f.families.entry(FontFamily::Monospace).or_default().push("phosphor".into());
    ctx.set_fonts(f);

    // Siempre oscuro, sin importar si Windows está en modo claro.
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.panel_fill = BG;
        s.visuals.window_fill = PANEL;
        s.visuals.extreme_bg_color = CARD;
        s.visuals.override_text_color = Some(TEXT);
        s.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
        s.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
        s.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        s.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        s.spacing.item_spacing = Vec2::new(10.0, 10.0);
        s.spacing.button_padding = Vec2::new(18.0, 9.0);
        s.interaction.selectable_labels = false;
        s.spacing.scroll = egui::style::ScrollStyle::floating(); // barra fina que aparece al pasar el mouse
        use egui::TextStyle::*;
        s.text_styles = [
            (Heading, bold(30.0)),
            (Body, regular(14.0)),
            (Button, semi(14.0)),
            (Small, regular(12.0)),
            (Monospace, mono(12.5)),
        ]
        .into();
    });
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Interruptor animado. `busy` lo muestra pulsando en naranja y sin clic.
pub fn toggle(ui: &mut Ui, key: &str, on: bool, busy: bool, enabled: bool) -> Response {
    let sense = if enabled && !busy { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(46.0, 25.0), sense);
    let t = ui.ctx().animate_bool(ui.make_persistent_id(("sw", key)), on);
    let (mut bg, mut knob) = (mix(Color32::from_rgb(42, 51, 80), BLUE, t), mix(Color32::from_rgb(216, 222, 234), Color32::WHITE, t));
    if busy {
        let pulse = (ui.input(|i| i.time) * 4.0).sin() as f32 * 0.5 + 0.5;
        bg = Color32::from_rgb(58, 50, 32);
        knob = mix(Color32::from_rgb(216, 222, 234), ACCENT, pulse);
        ui.ctx().request_repaint();
    }
    if !enabled {
        bg = bg.gamma_multiply(0.45);
        knob = knob.gamma_multiply(0.45);
    }
    let p = ui.painter();
    p.rect_filled(rect, 12.5, bg);
    let x = rect.left() + 12.5 + (rect.width() - 25.0) * t;
    p.circle_filled(Pos2::new(x, rect.center().y), 9.5, knob);
    if enabled && !busy { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Anillo de progreso (0-100) que arranca arriba y gira en sentido horario.
pub fn ring(ui: &Ui, center: Pos2, radius: f32, width: f32, pct: f32, color: Color32) {
    let p = ui.painter();
    p.circle_stroke(center, radius, Stroke::new(width, TRACK));
    let end = pct.clamp(0.0, 100.0) / 100.0 * std::f32::consts::TAU;
    if end <= 0.0 {
        return;
    }
    let n = 72;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + end * i as f32 / n as f32;
            center + radius * Vec2::new(a.cos(), a.sin())
        })
        .collect();
    let (first, last) = (pts[0], pts[n]);
    p.add(Shape::line(pts, Stroke::new(width, color)));
    p.circle_filled(first, width / 2.0, color); // puntas redondeadas
    p.circle_filled(last, width / 2.0, color);
}

pub fn card() -> egui::Frame {
    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(egui::Margin::same(18))
}
pub fn panel() -> egui::Frame {
    egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(14).inner_margin(egui::Margin::same(22))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Btn { Primary, Ghost, Danger }

pub fn button(ui: &mut Ui, text: &str, kind: Btn) -> Response {
    let (bg, fg) = match kind {
        Btn::Primary => (ACCENT, Color32::from_rgb(26, 18, 5)),
        Btn::Ghost => (CARD2, TEXT),
        Btn::Danger => (Color32::from_rgb(58, 29, 39), Color32::from_rgb(255, 138, 142)),
    };
    // Dibujado a mano: con fill fijo, egui no muestra ningún cambio al pasar el mouse.
    let galley = ui.painter().layout_no_wrap(text.to_owned(), semi(14.0), fg);
    let size = Vec2::new(galley.size().x + 2.0 * ui.spacing().button_padding.x, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let mut fill = if resp.is_pointer_button_down_on() { mix(bg, Color32::BLACK, 0.12) } else { mix(bg, Color32::WHITE, 0.12 * h) };
    let mut fg = fg;
    if !ui.is_enabled() {
        fill = fill.gamma_multiply(0.45);
        fg = fg.gamma_multiply(0.45);
    }
    ui.painter().rect_filled(rect, 9.0, fill);
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, fg);
    if ui.is_enabled() { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Etiqueta chica. Se pinta a mano para que mida exactamente lo que el texto, en cualquier layout.
pub fn badge(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    badge_font(ui, text, semi(11.0), fg, bg)
}
pub fn badge_font(ui: &mut Ui, text: &str, font: FontId, fg: Color32, bg: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(16.0, 7.0), Sense::hover());
    ui.painter().rect_filled(rect, 6.0, bg);
    ui.painter().galley(rect.min + Vec2::new(8.0, 3.5), galley, fg);
    resp
}

/// Columnas de igual ancho. A diferencia de ui.columns, no estira los botones ni justifica el texto.
pub fn columns<R>(ui: &mut Ui, n: usize, gap: f32, f: impl FnOnce(&mut [Ui]) -> R) -> R {
    let w = (ui.available_width() - gap * (n as f32 - 1.0)) / n as f32;
    let top = ui.cursor().min;
    let mut cols: Vec<Ui> = (0..n)
        .map(|i| {
            let rect = egui::Rect::from_min_size(top + Vec2::new(i as f32 * (w + gap), 0.0), Vec2::new(w, f32::INFINITY));
            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)))
        })
        .collect();
    let out = f(&mut cols);
    let h = cols.iter().map(|c| c.min_rect().height()).fold(0.0, f32::max);
    ui.allocate_space(Vec2::new(ui.available_width(), h));
    out
}

pub fn muted(text: impl Into<String>) -> RichText { RichText::new(text).color(MUTED) }
