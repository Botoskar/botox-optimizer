//! Las pantallas: Inicio, Optimizaciones, Debloat, Escáner, Boost-Up y Backups.
use crate::catalog::Tweak;
use crate::theme::*;
use crate::{sys, unix_now, Action, App, Page};
use eframe::egui::{self, Align, Align2, Color32, Label, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use egui_phosphor::regular as ic;
use serde_json::Value;

const CHIP_BG: Color32 = Color32::from_rgb(16, 23, 44);
const CARD_H: f32 = 236.0;

fn s(v: &Value) -> &str { v.as_str().unwrap_or_default() }
fn n(v: &Value) -> f64 { v.as_f64().unwrap_or(0.0) }

fn ago(unix: u64) -> String {
    let m = unix_now().saturating_sub(unix) / 60;
    match m { 0 => "recién".into(), 1..=59 => format!("hace {m} min"), 60..=1439 => format!("hace {} h", m / 60), _ => format!("hace {} d", m / 1440) }
}

/// "AMD Ryzen 5 5600 6-Core Processor" → "AMD Ryzen 5 5600".
fn cpu_short(name: &str) -> String {
    let base = name.split(" with ").next().unwrap_or(name);
    match base.find("-Core") {
        Some(i) => base[..i].rsplit_once(' ').map_or(base, |x| x.0).trim().to_string(),
        None => base.trim_end_matches(" Processor").trim().to_string(),
    }
}

fn title(ui: &mut Ui, text: &str) { ui.label(RichText::new(text).font(semi(16.0))); }

/// Encabezado de página: título y bajada a la izquierda, controles a la derecha.
/// Los controles se dibujan primero, así el texto usa solo el espacio que sobra y se parte en vez de quedar tapado.
fn header(ui: &mut Ui, name: &str, sub: &str, right: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 64.0), Layout::right_to_left(Align::Center), |ui| {
        right(ui);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.heading(name);
            ui.add(Label::new(muted(sub)).wrap());
        });
    });
    ui.add_space(14.0);
}

fn chip(ui: &mut Ui, label: &str, value: &str) {
    // Un Frame no salta de renglón solo: se mide y se baja a mano si no entra.
    let w = ui.fonts(|f| f.layout_no_wrap(label.into(), regular(12.0), MUTED).size().x + f.layout_no_wrap(value.into(), medium(12.0), TEXT).size().x) + 27.0;
    if ui.available_size_before_wrap().x < w {
        ui.end_row();
    }
    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(8).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.label(RichText::new(label).size(12.0).color(MUTED));
            ui.label(RichText::new(value).font(medium(12.0)));
        });
    });
}

fn kv(ui: &mut Ui, k: &str, v: &str) {
    if v.is_empty() {
        return;
    }
    let y = ui.cursor().top() - 3.0;
    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0_f32, Color32::from_rgb(31, 41, 68)));
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).size(13.0).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.add(Label::new(RichText::new(v).size(13.0)).wrap()));
    });
}

impl App {
    pub fn page(&mut self, ui: &mut Ui) {
        if let Some(e) = self.scan_err.clone() {
            ui.add_space(120.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No se pudo escanear la PC").font(semi(18.0)));
                ui.label(muted(e));
                if button(ui, "Reintentar", Btn::Primary).clicked() {
                    self.actions.push(Action::Rescan);
                }
            });
            return;
        }
        if self.spec.is_none() {
            ui.add_space(220.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(38.0).color(ACCENT));
                ui.add_space(8.0);
                ui.label(muted("Escaneando tu PC…"));
                ui.label(RichText::new("Leyendo el hardware y el estado de las optimizaciones").size(12.5).color(DIM));
            });
            return;
        }
        if let Some(k) = self.fake.clone() {
            let (que, tip) = match k.as_str() {
                "amd" => ("Ryzen 7 5700 + Radeon RX 7600", "Como la PC de un amigo con Radeon."),
                "x3d" => ("Ryzen 9 7950X3D + RX 7900 XTX, sin driver de V-Cache", "Mirá el Escáner y las pestañas Energía y Fps y latencia."),
                _ => ("Ryzen 9 7950X3D + RX 7900 XTX, V-Cache en orden", "Mirá el Escáner y las pestañas Energía y Fps y latencia."),
            };
            egui::Frame::new().fill(Color32::from_rgb(34, 27, 18)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(92, 62, 24))).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 9)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ic::FLASK).size(18.0).color(ACCENT));
                    ui.label(RichText::new(format!("Modo prueba: hardware simulado ({que})")).font(semi(14.0)));
                    ui.label(muted(format!("{tip} No se aplica nada en tu PC.")));
                });
            });
            ui.add_space(10.0);
        }
        if let Some(u) = self.update.clone() {
            egui::Frame::new().fill(Color32::from_rgb(34, 27, 18)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(92, 62, 24))).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 9)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if self.updating {
                        ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                        ui.label(RichText::new(format!("Instalando la versión {}…", u.version)).font(semi(14.0)));
                    } else {
                        ui.label(RichText::new(ic::ARROW_CIRCLE_UP).size(18.0).color(ACCENT));
                        ui.label(RichText::new(format!("Hay una versión nueva: {}", u.version)).font(semi(14.0)));
                        ui.label(muted(format!("(tenés la {})", env!("CARGO_PKG_VERSION"))));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if button(ui, "Actualizar", Btn::Primary).clicked() {
                                self.actions.push(Action::Update);
                            }
                        });
                    }
                });
            });
            ui.add_space(10.0);
        }
        if self.reboot_pending {
            egui::Frame::new().fill(Color32::from_rgb(20, 34, 67)).stroke(Stroke::new(1.0_f32, BLUE)).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 9)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ic::ARROWS_CLOCKWISE).size(18.0).color(BLUE));
                    ui.label("Hay cambios que necesitan reiniciar la PC para tomar efecto.");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, "Reiniciar ahora", Btn::Ghost).clicked() {
                            self.actions.push(Action::Reboot);
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }
        if let Some((total, ..)) = self.bulk {
            // Progreso de los cambios en lote.
            let inflight = self.busy.iter().filter(|b| !b.starts_with("b:") && *b != "rp").count();
            let done = total.saturating_sub(self.queue.len() + inflight);
            egui::Frame::new().fill(Color32::from_rgb(34, 27, 18)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(92, 62, 24))).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 11)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(16.0).color(ACCENT));
                    ui.label(RichText::new(format!("Aplicando cambios: {done} de {total}")).font(semi(14.0)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(360.0), 8.0), Sense::hover());
                        ui.painter().rect_filled(r, 4.0, TRACK);
                        let w = r.width() * done as f32 / total.max(1) as f32;
                        ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(w, 8.0)), 4.0, ACCENT);
                    });
                });
            });
            ui.add_space(10.0);
        }
        match self.page {
            Page::Home => self.home(ui),
            Page::Scan => self.scan_page(ui),
            Page::Tweaks => self.tweak_page(ui, "Optimizaciones", "ajustes de FPS, latencia, red, registro y privacidad. Todos se pueden revertir.", &[]),
            Page::Debloat => self.tweak_page(ui, "Debloat", "Quitá apps preinstaladas y servicios que corren de fondo sin que los uses.", &["debloat", "svc"]),
            Page::Boost => self.boost_page(ui),
            Page::Startup => self.startup_page(ui),
            Page::Backup => self.backup_page(ui),
        }
    }

    // ── Arranque ──

    /// Programas que arrancan con Windows, con un interruptor cada uno. Apagar no borra nada.
    fn startup_page(&mut self, ui: &mut Ui) {
        header(ui, "Inicio de Windows", "Programas que arrancan al encender la PC. Apagá los que no uses para que inicie más rápido: no se borra nada y podés volver a encenderlos.", |ui| {
            if button(ui, "Actualizar", Btn::Ghost).clicked() {
                self.actions.push(Action::RescanStartup);
            }
        });
        if self.startup.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(ic::POWER).size(40.0).color(DIM));
                ui.label(muted("No hay programas configurados para arrancar con Windows."));
            });
            return;
        }
        let on = self.startup.iter().filter(|i| i.enabled).count();
        ui.label(muted(format!("{} programas · {on} arrancan con Windows", self.startup.len())));
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        for i in 0..self.startup.len() {
            let it = self.startup[i].clone();
            egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
                let size = Vec2::new(ui.available_width(), 40.0);
                ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    if toggle(ui, &format!("su{i}{}", it.name), it.enabled, false, true).on_hover_text(if it.enabled { "Apagar al iniciar" } else { "Encender al iniciar" }).clicked() {
                        self.actions.push(Action::Startup(i));
                    }
                    badge(ui, it.place.label(), MUTED, CHIP_BG);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add(Label::new(RichText::new(&it.name).font(semi(14.0)).color(if it.enabled { TEXT } else { DIM })).truncate());
                        ui.add(Label::new(RichText::new(&it.command).size(12.5).color(MUTED)).truncate());
                    });
                });
            });
        }
    }

    /// Interruptor de un tweak. Mientras no se conoce su estado (carga de fondo) o está aplicándose, pulsa en naranja.
    fn switch(&mut self, ui: &mut Ui, t: &Tweak) {
        let busy = self.busy.contains(&t.id) || !self.state.contains_key(&t.id);
        let tip = if self.on(&t.id) { "Revertir" } else { "Activar" };
        if toggle(ui, &t.id, self.on(&t.id), busy, t.applies(&self.facts)).on_hover_text(tip).clicked() {
            self.actions.push(Action::Toggle(t.id.clone()));
        }
    }

    /// Fila compacta: nombre, descripción en una línea e interruptor.
    fn row(&mut self, ui: &mut Ui, t: &Tweak, show_why: bool) {
        egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
            let why = if show_why { t.reason(&self.facts) } else { String::new() };
            let size = Vec2::new(ui.available_width(), if why.is_empty() { 40.0 } else { 58.0 });
            ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
                self.switch(ui, t);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(Label::new(RichText::new(&t.name).font(semi(14.0))).truncate());
                    ui.add(Label::new(RichText::new(&t.desc).size(12.5).color(MUTED)).truncate());
                    if !why.is_empty() {
                        ui.label(RichText::new(&why).size(12.0).color(ACCENT2));
                    }
                });
            });
        });
    }

    // ── Inicio ──

    fn home(&mut self, ui: &mut Ui) {
        let (cat, f, spec) = (self.cat.clone(), self.facts.clone(), self.spec.clone().unwrap());
        let usable: Vec<&Tweak> = cat.tweaks.iter().filter(|t| (t.cat != "laptop" || f.laptop) && t.applies(&f)).collect();
        let active: Vec<&Tweak> = usable.iter().copied().filter(|t| self.on(&t.id)).collect();
        let pending: Vec<&Tweak> = usable.iter().copied().filter(|t| t.recommended(&f) && !self.on(&t.id)).collect();
        let pct = if usable.is_empty() { 0.0 } else { (100.0 * active.len() as f32 / usable.len() as f32).round() };

        if !self.admin {
            egui::Frame::new().fill(Color32::from_rgb(46, 30, 16)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(90, 58, 20))).corner_radius(12).inner_margin(egui::Margin::symmetric(18, 13)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Modo solo lectura.").font(semi(14.0)).color(ACCENT2));
                    ui.label("Abrí Botox Optimizer como administrador para aplicar optimizaciones.");
                });
            });
            ui.add_space(10.0);
        }
        columns(ui, 2, 22.0, |c| {
            // Columna izquierda
            let ui = &mut c[0];
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(150.0), Sense::hover());
                // Resplandor suave detrás del anillo; el porcentaje sube animado cuando cambia.
                for i in 0..12 {
                    ui.painter().circle_filled(rect.center(), 84.0 - i as f32 * 5.0, ACCENT.gamma_multiply(0.012));
                }
                let shown = ui.ctx().animate_value_with_time(egui::Id::new("pct"), pct, 0.8);
                ring(ui, rect.center(), 66.0, 13.0, shown, ACCENT);
                self.floating_logo(ui, rect.center(), 116.0);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(22.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new(format!("{shown:.0}% Optimizado")).font(bold(32.0)));
                    ui.label(muted(if !self.ps_ready { "Terminando de leer el estado de tu PC…" } else if pct >= 85.0 { "Tu PC está en muy buena forma" } else { "Tu PC puede rendir más" }));
                    ui.add_space(6.0);
                    egui::Frame::new().fill(CHIP_BG).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(RichText::new(pending.len().to_string()).font(semi(13.0)).color(ACCENT));
                            ui.label(RichText::new("recomendadas para tu PC").size(13.0).color(MUTED));
                        });
                    });
                });
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                chip(ui, "CPU", &cpu_short(s(&spec["cpu"]["name"])));
                chip(ui, "GPU", s(&sys::main_gpu(&spec)["name"]));
                chip(ui, "RAM", &format!("{} GB", f.ram_gb));
                chip(ui, "C:", &format!("{} GB libres", f.c_free_gb));
            });
            ui.add_space(10.0);
            let st = self.stats.clone();
            columns(ui, 3, 12.0, |c| {
                let stat = |ui: &mut Ui, label: &str, pct: Option<u32>, color: Color32, value: String| {
                    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(12).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let gauge = |ui: &mut Ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(62.0), Sense::hover());
                            ring(ui, r.center(), 27.0, 7.0, pct.unwrap_or(0) as f32, color);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, pct.map_or("–".into(), |p| format!("{p}%")), semi(13.0), TEXT);
                        };
                        let text = |ui: &mut Ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new(label).size(12.0).color(MUTED));
                            ui.add(Label::new(RichText::new(&value).font(semi(13.0))).wrap());
                        };
                        // En ventanas angostas el texto va debajo del anillo para no partirse letra por letra.
                        if ui.available_width() < 150.0 {
                            ui.vertical_centered(|ui| {
                                gauge(ui);
                                text(ui);
                            });
                        } else {
                            ui.horizontal(|ui| {
                                gauge(ui);
                                ui.vertical(|ui| {
                                    ui.add_space(6.0);
                                    text(ui);
                                });
                            });
                        }
                    });
                };
                stat(&mut c[0], "CPU", st.cpu, GREEN, format!("{} hilos", n(&spec["cpu"]["threads"])));
                stat(&mut c[1], "GPU", st.gpu.map(|g| g.0), BLUE, st.gpu.map_or("Sin datos".into(), |g| format!("{}°C · VRAM {}%", g.2, g.1)));
                stat(&mut c[2], "RAM", Some(st.ram), ACCENT, format!("{:.1} / {} GB", st.ram_used_gb, f.ram_gb));
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                title(ui, "Tus optimizaciones activas");
                ui.label(RichText::new(active.len().to_string()).size(16.0).color(DIM));
            });
            if active.is_empty() {
                ui.label(muted("Todavía ninguna."));
            }
            for t in active.iter().take(6) {
                self.row(ui, t, false);
            }
            if active.len() > 6 && button(ui, &format!("Ver las {}", active.len()), Btn::Ghost).clicked() {
                self.actions.push(Action::Go(Page::Tweaks, Some("active")));
            }

            // Columna derecha
            let ui = &mut c[1];
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ic::LIGHTNING).size(20.0).color(ACCENT));
                    title(ui, "Recomendaciones para tu PC");
                });
                if pending.is_empty() {
                    ui.label(muted(if self.ps_ready { "Aplicaste todas las recomendaciones." } else { "Cargando…" }));
                }
                for t in pending.iter().take(5) {
                    self.row(ui, t, true);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if !pending.is_empty() && button(ui, &format!("Aplicar las {} recomendadas", pending.len()), Btn::Primary).clicked() {
                        self.actions.push(Action::Many(pending.iter().map(|t| t.id.clone()).collect(), "aplicar"));
                    }
                    if button(ui, "Ver todas", Btn::Ghost).clicked() {
                        self.actions.push(Action::Go(Page::Tweaks, Some("rec")));
                    }
                });
            });
            ui.add_space(12.0);
            title(ui, "Atajos");
            columns(ui, 4, 12.0, |c| {
                for (i, (page, icon, name)) in [(Page::Scan, ic::CPU, "Escáner"), (Page::Boost, ic::GAUGE, "Boost-Up"), (Page::Debloat, ic::TRASH, "Debloat"), (Page::Backup, ic::CLOCK_COUNTER_CLOCKWISE, "Backups")].into_iter().enumerate() {
                    let ui = &mut c[i];
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 108.0), Sense::click());
                    let hover = resp.hovered();
                    let rect = if hover { rect.translate(Vec2::new(0.0, -2.0)) } else { rect };
                    ui.painter().rect(rect, 12.0, CARD, Stroke::new(1.0_f32, if hover { ACCENT } else { LINE }), StrokeKind::Inside);
                    ui.painter().text(rect.center() - Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, icon, regular(36.0), TEXT);
                    ui.painter().text(rect.center() + Vec2::new(0.0, 28.0), Align2::CENTER_CENTER, name, semi(14.0), if hover { TEXT } else { MUTED });
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.actions.push(Action::Go(page, None));
                    }
                }
            });
        });
    }

    // ── Optimizaciones y Debloat ──

    fn tweak_page(&mut self, ui: &mut Ui, name: &str, sub: &str, fixed: &[&str]) {
        let (cat, f) = (self.cat.clone(), self.facts.clone());
        let base: Vec<&Tweak> = cat.tweaks.iter().filter(|t| (t.cat != "laptop" || f.laptop) && (fixed.is_empty() || fixed.contains(&t.cat.as_str()))).collect();
        let mut tabs: Vec<(String, String)> = vec![("all".into(), "Todas".into())];
        if fixed.is_empty() {
            tabs.extend([("rec".to_string(), "Recomendadas".to_string()), ("active".into(), "Activas".into())]);
        }
        tabs.extend(cat.cats.iter().filter(|(k, _)| (fixed.is_empty() || fixed.contains(&k.as_str())) && (k.as_str() != "laptop" || f.laptop)).map(|(k, v)| (k.clone(), s(v).to_string())));
        if !tabs.iter().any(|t| t.0 == self.tab) {
            self.tab = "all".into();
        }
        let in_tab = |t: &Tweak, k: &str, app: &App| match k { "all" => true, "rec" => t.recommended(&f), "active" => app.on(&t.id), c => t.cat == c };
        let q = self.query.to_lowercase();
        let list: Vec<&Tweak> = base.iter().copied().filter(|t| in_tab(t, &self.tab, self) && (q.is_empty() || format!("{} {}", t.name, t.desc).to_lowercase().contains(&q))).collect();
        // Activar en lote: solo bajo riesgo. El debloat solo desde su propia página.
        let bulk: Vec<String> = list.iter().filter(|t| t.applies(&f) && self.state.get(&t.id) == Some(&false) && t.risk == "low" && (!t.debloat || !fixed.is_empty())).map(|t| t.id.clone()).collect();

        let sub = if fixed.is_empty() { format!("{} {sub}", base.len()) } else { sub.to_string() };
        header(ui, name, &sub, |ui| {
            if !bulk.is_empty() && button(ui, &format!("Activar {} de bajo riesgo", bulk.len()), Btn::Primary).clicked() {
                self.actions.push(Action::Many(bulk.clone(), "activar"));
            }
            self.search(ui);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(6.0);
            ui.spacing_mut().button_padding = Vec2::new(14.0, 6.0);
            for (k, label) in &tabs {
                let on = *k == self.tab;
                let count = base.iter().filter(|t| in_tab(t, k, self)).count();
                let text = RichText::new(format!("{label}  {count}")).font(regular(13.0)).color(if on { ACCENT } else { MUTED });
                let b = egui::Button::new(text).fill(if on { Color32::from_rgb(34, 25, 14) } else { CARD }).stroke(Stroke::new(1.0_f32, if on { ACCENT } else { LINE })).corner_radius(8);
                if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.tab = k.clone();
                }
            }
        });
        ui.add_space(8.0);
        if list.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if q.is_empty() { ic::CHECK_CIRCLE } else { ic::MAGNIFYING_GLASS }).size(44.0).color(DIM));
                if q.is_empty() {
                    ui.label(muted("Nada que mostrar en esta pestaña."));
                } else {
                    ui.label(muted(format!("No hay optimizaciones con \"{}\".", self.query)));
                    if button(ui, "Limpiar búsqueda", Btn::Ghost).clicked() {
                        self.query.clear();
                    }
                }
            });
        }
        let total = ui.available_width();
        let cols = (((total + 14.0) / 344.0).floor() as usize).max(1);
        let w = (total - 14.0 * (cols as f32 - 1.0)) / cols as f32;
        ui.spacing_mut().item_spacing = Vec2::splat(14.0);
        for chunk in list.chunks(cols) {
            ui.horizontal(|ui| {
                for t in chunk {
                    self.card(ui, t, w);
                }
            });
        }
    }

    /// Buscador con ícono, borde naranja al escribir y botón para borrar. Ctrl+F lo enfoca.
    fn search(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("search");
        if std::mem::take(&mut self.focus_search) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let focused = ui.memory(|m| m.has_focus(id));
        egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, if focused { ACCENT } else { LINE })).corner_radius(10).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
            // De derecha a izquierda (sigue al encabezado): primero la X, después el texto, después la lupa.
            ui.allocate_ui_with_layout(Vec2::new(272.0, 20.0), Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (x, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                if !self.query.is_empty() {
                    ui.painter().text(x.center(), Align2::CENTER_CENTER, ic::X_CIRCLE, regular(17.0), if resp.hovered() { TEXT } else { MUTED });
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Borrar (Esc)").clicked() {
                        self.query.clear();
                    }
                }
                ui.add(egui::TextEdit::singleline(&mut self.query).id(id).frame(false).hint_text("Buscar…  Ctrl+F").desired_width(220.0).margin(egui::Margin::ZERO).font(regular(14.0)));
                ui.label(RichText::new(ic::MAGNIFYING_GLASS).size(16.0).color(if focused { ACCENT } else { MUTED }));
            });
        });
    }

    fn card(&mut self, ui: &mut Ui, t: &Tweak, w: f32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, CARD_H), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let (on, ok) = (self.on(&t.id), t.applies(&self.facts));
        // Al pasar el mouse la tarjeta se aclara un poco; las activas tienen borde azul y la marca "Activo".
        let h = ui.ctx().animate_bool_with_time(egui::Id::new(("card", &t.id)), ui.rect_contains_pointer(rect), 0.12);
        let edge = if on { Color32::from_rgb(42, 63, 122) } else { LINE };
        ui.painter().rect(rect, 12.0, mix(CARD, Color32::from_rgb(26, 36, 64), h), Stroke::new(1.0_f32, mix(edge, Color32::from_rgb(72, 90, 140), h)), StrokeKind::Inside);
        if on {
            ui.painter().text(rect.right_top() + Vec2::new(-20.0, 26.0), Align2::RIGHT_CENTER, format!("{}  Activo", ic::CHECK_CIRCLE), medium(12.0), GREEN);
        }
        let foot_y = rect.bottom() - 50.0;
        ui.painter().hline(rect.x_range(), foot_y, Stroke::new(1.0_f32, LINE));

        let body_rect = Rect::from_min_max(rect.min + Vec2::new(20.0, 16.0), Pos2::new(rect.right() - 20.0, foot_y - 6.0));
        let mut body = ui.new_child(UiBuilder::new().max_rect(body_rect).layout(Layout::top_down(Align::Min)));
        body.set_clip_rect(body_rect.intersect(ui.clip_rect()));
        body.spacing_mut().item_spacing.y = 6.0;
        badge(&mut body, self.cat.cat_name(&t.cat), MUTED, CHIP_BG);
        body.add(Label::new(RichText::new(&t.name).font(semi(16.0))).wrap());
        body.add(Label::new(RichText::new(&t.desc).size(13.0).color(MUTED)).wrap());
        let why = t.reason(&self.facts);
        if !ok {
            body.label(RichText::new("No aplica a esta PC").size(12.0).color(DIM));
        } else if !why.is_empty() {
            body.label(RichText::new(format!("● {why}")).size(12.0).color(ACCENT2));
        }

        let foot_rect = Rect::from_min_max(Pos2::new(rect.left() + 20.0, foot_y), Pos2::new(rect.right() - 20.0, rect.bottom()));
        let mut foot = ui.new_child(UiBuilder::new().max_rect(foot_rect).layout(Layout::right_to_left(Align::Center)));
        foot.set_clip_rect(foot_rect.intersect(ui.clip_rect()));
        self.switch(&mut foot, t);
        foot.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if t.recommended(&self.facts) {
                badge(ui, "Recomendado", ACCENT, Color32::from_rgb(44, 30, 11));
            }
            if t.risk != "low" {
                badge(ui, "Riesgo medio", YELLOW, Color32::from_rgb(51, 41, 12));
            }
            // Registro y Reinicio van como íconos con explicación al pasar el mouse, para que siempre entren.
            let icon = |ui: &mut Ui, glyph: &str, fg: Color32, bg: Color32, tip: &str| {
                badge_font(ui, glyph, regular(14.0), fg, bg).on_hover_text(tip);
            };
            if t.uses_reg {
                icon(ui, ic::DATABASE, Color32::from_rgb(127, 162, 255), Color32::from_rgb(20, 34, 67), "Cambia valores del registro de Windows");
            }
            if t.reboot {
                icon(ui, ic::ARROWS_CLOCKWISE, MUTED, Color32::from_rgb(28, 34, 54), "Necesita reiniciar la PC para tomar efecto");
            }
        });
    }

    // ── Escáner ──

    fn scan_page(&mut self, ui: &mut Ui) {
        let (spec, f) = (self.spec.clone().unwrap(), self.facts.clone());
        header(ui, "Escáner del sistema", &format!("{} · el hardware y la configuración que detectamos", s(&spec["pc"])), |ui| {
            if button(ui, "Volver a escanear", Btn::Ghost).clicked() {
                self.actions.push(Action::Rescan);
            }
        });
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            title(ui, "Hallazgos");
            for (i, (lvl, t, d)) in sys::findings(&spec, &f).into_iter().enumerate() {
                if i > 0 {
                    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top() - 4.0, Stroke::new(1.0_f32, LINE));
                }
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::new(12.0, 20.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 5.0, match lvl { "ok" => GREEN, "bad" => RED, _ => YELLOW });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(t).font(semi(14.0)));
                        ui.add(Label::new(muted(d)).wrap());
                    });
                });
            }
        });
        ui.add_space(8.0);

        // Cada tarjeta es (ícono, tipo, nombre, filas, barras de uso de volúmenes).
        type SpecCard = (&'static str, &'static str, String, Vec<(String, String)>, Vec<(String, String, f32)>);
        let mut cards: Vec<SpecCard> = vec![];
        let cpu = &spec["cpu"];
        let row = |k: &str, v: String| (k.to_string(), v);
        cards.push((ic::CPU, "Procesador", s(&cpu["name"]).into(), vec![
            row("Núcleos / hilos", format!("{} / {}", n(&cpu["cores"]), n(&cpu["threads"]))),
            row("Frecuencia base", format!("{:.2} GHz", n(&cpu["mhz"]) / 1000.0)),
            row("Caché L3", if n(&cpu["l3MB"]) > 0.0 { format!("{} MB", n(&cpu["l3MB"])) } else { String::new() }),
            row("Fabricante", s(&cpu["maker"]).into()),
        ], vec![]));
        let mut gpus: Vec<&Value> = spec["gpus"].as_array().into_iter().flatten().filter(|g| n(&g["vramGB"]) > 0.0).collect();
        gpus.sort_by(|a, b| n(&b["vramGB"]).total_cmp(&n(&a["vramGB"])));
        for g in gpus {
            cards.push((ic::GRAPHICS_CARD, "Placa de video", s(&g["name"]).into(), vec![
                row("VRAM", format!("{} GB", n(&g["vramGB"]))),
                row("Driver", s(&g["driver"]).into()),
                row("Resolución", if n(&g["refresh"]) > 0.0 { format!("{} @ {} Hz", s(&g["res"]), n(&g["refresh"])) } else { String::new() }),
            ], vec![]));
        }
        let mods = spec["ram"]["modules"].as_array().cloned().unwrap_or_default();
        let mut rows = vec![row("Módulos", mods.len().to_string())];
        rows.extend(mods.iter().map(|m| row(s(&m["slot"]), format!("{} GB · {} MHz · {} {}", n(&m["gb"]), if n(&m["conf"]) > 0.0 { n(&m["conf"]) } else { n(&m["speed"]) }, s(&m["maker"]), s(&m["part"])))));
        cards.push((ic::MEMORY, "Memoria", format!("{} GB", n(&spec["ram"]["gb"])), rows, vec![]));
        let disks = spec["disks"].as_array().cloned().unwrap_or_default();
        let total: f64 = disks.iter().map(|d| n(&d["sizeGB"])).sum();
        let rows = disks.iter().map(|d| {
            let letters: Vec<String> = d["letters"].as_array().into_iter().flatten().map(|l| format!("{}:", s(l))).collect();
            let kind = if s(&d["bus"]) == "NVMe" { "NVMe" } else { s(&d["media"]) };
            row(s(&d["name"]), format!("{kind} · {} GB {}", n(&d["sizeGB"]), if letters.is_empty() { String::new() } else { format!("· {}", letters.join(" ")) }))
        }).collect();
        let bars = spec["vols"].as_array().into_iter().flatten().map(|v| (format!("{}: {}", s(&v["letter"]), s(&v["label"])), format!("{} / {} GB libres", n(&v["freeGB"]), n(&v["sizeGB"])), 1.0 - (n(&v["freeGB"]) / n(&v["sizeGB"]).max(1.0)) as f32)).collect();
        cards.push((ic::HARD_DRIVES, "Almacenamiento", format!("{} en {} discos", if total >= 1000.0 { format!("{:.2} TB", total / 1000.0) } else { format!("{total} GB") }, disks.len()), rows, bars));
        cards.push((ic::CIRCUITRY, "Placa madre", s(&spec["board"]).into(), vec![row("BIOS", s(&spec["bios"]).into()), row("Tipo", if f.laptop { "Notebook" } else { "Escritorio" }.into())], vec![]));
        let os = &spec["os"];
        cards.push((ic::WINDOWS_LOGO, "Sistema", s(&os["name"]).into(), vec![
            row("Versión", format!("{} (build {})", s(&os["ver"]), s(&os["build"]))),
            row("Encendida hace", format!("{} h", n(&os["uptimeH"]))),
            row("Plan de energía", s(&spec["plan"]).into()),
            row("VBS", match sys::vbs_state(&spec) { sys::Vbs::On(_) | sys::Vbs::Legacy => "Activa", sys::Vbs::Idle => "Sin funciones activas", sys::Vbs::Off => "Inactiva" }.into()),
            row("HAGS", if n(&spec["hags"]) == 2.0 { "Activado" } else { "Desactivado" }.into()),
            row("Administrador", if self.admin { "Sí" } else { "No" }.into()),
        ], vec![]));
        for nic in spec["nics"].as_array().into_iter().flatten() {
            cards.push((ic::WIFI_HIGH, "Red", s(&nic["desc"]).into(), vec![
                row("Conexión", if nic["wifi"].as_bool() == Some(true) { "Wi-Fi" } else { "Cable" }.into()),
                row("Velocidad", s(&nic["speed"]).into()),
                row("DNS", s(&nic["dns"]).into()),
            ], vec![]));
        }

        let cols = if ui.available_width() >= 1150.0 { 3 } else { 2 };
        columns(ui, cols, 14.0, |c| {
            for (i, (icon, kind, name, rows, bars)) in cards.iter().enumerate() {
                let ui = &mut c[i % cols];
                card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
                        ui.painter().rect_filled(r, 10.0, Color32::from_rgb(34, 25, 14));
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, *icon, regular(22.0), ACCENT);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(kind.to_uppercase()).size(11.5).color(MUTED));
                            ui.add(Label::new(RichText::new(name).font(semi(15.0))).wrap());
                        });
                    });
                    ui.add_space(2.0);
                    for (k, v) in rows {
                        kv(ui, k, v);
                    }
                    for (k, v, used) in bars {
                        kv(ui, k, v);
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
                        ui.painter().rect_filled(r, 3.0, TRACK);
                        ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * used.clamp(0.0, 1.0), 6.0)), 3.0, if *used > 0.9 { RED } else { ACCENT });
                    }
                });
                ui.add_space(4.0);
            }
        });
    }

    // ── Boost-Up ──

    fn boost_page(&mut self, ui: &mut Ui) {
        header(ui, "Boost-Up", "Limpiezas y mantenimiento para correr cada tanto", |_| {});
        let cat = self.cat.clone();
        columns(ui, 2, 22.0, |c| {
            let ui = &mut c[0];
            for (i, b) in cat.boosts.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), Sense::click());
                let sel = i == self.boost_sel;
                let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
                ui.painter().rect(rect, 12.0, mix(CARD, Color32::from_rgb(26, 36, 64), h), Stroke::new(1.0_f32, if sel { ACCENT } else { mix(LINE, Color32::from_rgb(72, 90, 140), h) }), StrokeKind::Inside);
                if sel {
                    ui.painter().rect_filled(Rect::from_min_size(rect.min + Vec2::new(0.0, 12.0), Vec2::new(3.0, 42.0)), 2.0, ACCENT);
                }
                let icon = match b.id.as_str() { "temp" => ic::BROOM, "shaders" => ic::GAME_CONTROLLER, "thumbs" => ic::IMAGE, "dumps" => ic::BUG, "wucache" => ic::PACKAGE, "recent" => ic::FILE_TEXT, "dns" => ic::GLOBE, "trim" => ic::HARD_DRIVES, _ => ic::PUZZLE_PIECE };
                let p = ui.painter();
                p.text(rect.left_center() + Vec2::new(40.0, 0.0), Align2::CENTER_CENTER, icon, regular(28.0), if sel { ACCENT } else { TEXT });
                let last = self.boost_last(&b.id);
                p.text(rect.left_center() + Vec2::new(78.0, -11.0), Align2::LEFT_CENTER, last.map_or("Recomendado".to_string(), |t| format!("Última vez: {}", ago(t))), regular(12.0), MUTED);
                p.text(rect.left_center() + Vec2::new(78.0, 9.0), Align2::LEFT_CENTER, &b.name, semi(14.0), TEXT);
                if self.busy.contains(&format!("b:{}", b.id)) {
                    egui::Spinner::new().size(18.0).color(ACCENT).paint_at(ui, Rect::from_center_size(rect.right_center() - Vec2::new(30.0, 0.0), Vec2::splat(18.0)));
                } else if last.is_some() {
                    p.text(rect.right_center() - Vec2::new(18.0, 0.0), Align2::RIGHT_CENTER, format!("Hecho  {}", ic::CHECK_CIRCLE), regular(13.0), ACCENT);
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.boost_sel = i;
                }
            }
            let ui = &mut c[1];
            let b = &cat.boosts[self.boost_sel.min(cat.boosts.len() - 1)];
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(420.0);
                ui.add(Label::new(RichText::new(&b.name).font(bold(30.0))).wrap());
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top() + 2.0, Stroke::new(1.0_f32, LINE));
                ui.add_space(12.0);
                ui.add(Label::new(RichText::new(&b.desc).size(14.0).color(Color32::from_rgb(200, 208, 226))).wrap());
                ui.label(muted(format!("Cuándo: {}", b.every)));
                if let Some(out) = self.boost_out.get(&b.id) {
                    egui::Frame::new().fill(CHIP_BG).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(10).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(out).color(GREEN));
                    });
                }
                ui.add_space(14.0);
                let running = self.busy.contains(&format!("b:{}", b.id));
                ui.add_enabled_ui(!running, |ui| {
                    ui.spacing_mut().button_padding = Vec2::new(60.0, 12.0);
                    if button(ui, if running { "Ejecutando…" } else { "Ejecutar" }, Btn::Primary).clicked() {
                        self.actions.push(Action::RunBoost(b.id.clone()));
                    }
                });
            });
        });
    }

    // ── Backups ──

    fn backup_page(&mut self, ui: &mut Ui) {
        let creating = self.busy.contains("rp");
        header(ui, "Backups", "Cada cambio guarda antes los valores originales.", |ui| {
            if button(ui, if creating { "Creando…" } else { "Crear punto de restauración" }, Btn::Primary).clicked() {
                self.actions.push(Action::CreateRestore);
            }
            if button(ui, "Restaurar sistema…", Btn::Ghost).clicked() {
                self.actions.push(Action::OpenRestore);
            }
        });
        let table_head = |ui: &mut Ui, cols: [&str; 2]| {
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
            ui.painter().rect_filled(r, 10.0, CARD2);
            ui.painter().text(r.left_center() + Vec2::new(16.0, 0.0), Align2::LEFT_CENTER, cols[0], semi(14.0), TEXT);
            ui.painter().text(Pos2::new(r.left() + r.width() * 0.55, r.center().y), Align2::LEFT_CENTER, cols[1], semi(14.0), TEXT);
        };
        let table_row = |ui: &mut Ui, a: &str, sub: &str, b: &str| -> Rect {
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::hover());
            ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0_f32, LINE));
            let dy = if sub.is_empty() { 0.0 } else { -8.0 };
            ui.painter().text(r.left_center() + Vec2::new(16.0, dy), Align2::LEFT_CENTER, a, regular(14.0), TEXT);
            if !sub.is_empty() {
                ui.painter().text(r.left_center() + Vec2::new(16.0, 10.0), Align2::LEFT_CENTER, sub, regular(12.0), MUTED);
            }
            ui.painter().text(Pos2::new(r.left() + r.width() * 0.55, r.center().y), Align2::LEFT_CENTER, b, regular(14.0), TEXT);
            r
        };
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            title(ui, "Puntos de restauración");
            ui.add_space(12.0);
            match &self.restore {
                None => { ui.add(egui::Spinner::new().color(ACCENT)); }
                Some(Err(_)) => { ui.label(muted("Se necesita administrador para listar los puntos.")); }
                Some(Ok(list)) if list.is_empty() => { ui.add(Label::new(muted("No hay puntos de restauración. Si la protección del sistema está apagada, activala en \"Crear un punto de restauración\" de Windows.")).wrap()); }
                Some(Ok(list)) => {
                    table_head(ui, ["Descripción", "Fecha"]);
                    for (d, t) in list {
                        table_row(ui, d, "", t);
                    }
                }
            }
        });
        ui.add_space(10.0);
        let (cat, ids) = (self.cat.clone(), self.reversible());
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                title(ui, "Cambios hechos por Botox Optimizer");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, "Revertir todo", Btn::Danger).clicked() {
                        self.actions.push(Action::RevertAll);
                    }
                });
            });
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(8.0);
            if ids.is_empty() {
                ui.label(muted("Todavía no aplicaste cambios desde Botox Optimizer."));
                return;
            }
            table_head(ui, ["Optimización", "Respaldo"]);
            for id in &ids {
                let Some(t) = cat.tweak(id) else { continue };
                let when = self.backups.iter().find(|b| b.0 == *id).map(|b| chrono::DateTime::<chrono::Local>::from(b.1).format("%d/%m/%Y %H:%M").to_string());
                let r = table_row(ui, &t.name, cat.cat_name(&t.cat), when.as_deref().unwrap_or("Se revierte al valor de Windows"));
                let mut right = ui.new_child(UiBuilder::new().max_rect(r.shrink2(Vec2::new(16.0, 0.0))).layout(Layout::right_to_left(Align::Center)));
                self.switch(&mut right, t);
            }
        });
    }
}
