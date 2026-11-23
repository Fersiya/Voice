use crate::capture::Capturer;
use crate::config::{Config, Engine, Region, Rule};
use crate::state::Shared;
use crate::tts::Utterance;
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use image::RgbaImage;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(13, 15, 22);
const PANEL: Color32 = Color32::from_rgb(20, 23, 33);
const CARD: Color32 = Color32::from_rgb(27, 31, 44);
const ACCENT: Color32 = Color32::from_rgb(124, 92, 255);
const OK: Color32 = Color32::from_rgb(61, 220, 151);
const WARN: Color32 = Color32::from_rgb(255, 107, 107);
const MUTED: Color32 = Color32::from_rgb(140, 148, 170);

enum Picker {
    Idle,
    /// Ждём снимок экрана (успеть переключиться на игру).
    Waiting { until: Instant },
    Selecting { tex: TextureHandle, size: [u32; 2], start: Option<egui::Pos2>, cur: Option<egui::Pos2> },
}

pub struct App {
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Utterance>,
    started: Instant,

    preview_tex: Option<TextureHandle>,
    preview_gen: u64,

    picker: Picker,
    pick_rx: Option<Receiver<Result<RgbaImage, String>>>,

    saved_text: String,
    dirty_since: Option<Instant>,
}

impl App {
    pub fn new(ctx: &egui::Context, shared: Arc<Mutex<Shared>>, tx: Sender<Utterance>) -> Self {
        apply_style(ctx);
        let saved_text = shared.lock().unwrap().cfg.to_toml();
        Self {
            shared,
            tx,
            started: Instant::now(),
            preview_tex: None,
            preview_gen: 0,
            picker: Picker::Idle,
            pick_rx: None,
            saved_text,
            dirty_since: None,
        }
    }

    fn start_picker(&mut self, ctx: &egui::Context, monitor: usize, window: String) {
        let (tx, rx) = mpsc::channel();
        self.pick_rx = Some(rx);
        self.picker = Picker::Waiting { until: Instant::now() + Duration::from_secs(4) };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            let res = Capturer::new(monitor, &window)
                .and_then(|c| c.grab())
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(33));
        let shared = self.shared.clone();
        let mut sh = shared.lock().unwrap();

        // Снимок экрана для выбора области готов.
        if let Some(rx) = &self.pick_rx {
            if let Ok(res) = rx.try_recv() {
                self.pick_rx = None;
                match res {
                    Ok(img) => {
                        let size = [img.width(), img.height()];
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [size[0] as usize, size[1] as usize],
                            img.as_raw(),
                        );
                        let tex = ctx.load_texture("screen", color, TextureOptions::LINEAR);
                        self.picker = Picker::Selecting { tex, size, start: None, cur: None };
                    }
                    Err(e) => {
                        sh.error = Some(e);
                        self.picker = Picker::Idle;
                    }
                }
            }
        }

        // Превью области.
        if sh.preview_gen != self.preview_gen {
            self.preview_gen = sh.preview_gen;
            if let Some(img) = &sh.preview {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                match &mut self.preview_tex {
                    Some(t) => t.set(color, TextureOptions::LINEAR),
                    None => {
                        self.preview_tex = Some(ctx.load_texture("preview", color, TextureOptions::LINEAR))
                    }
                }
            }
        }

        self.top_bar(ctx, &mut sh);
        self.settings_panel(ctx, &mut sh);
        self.main_panel(ctx, &sh);

        // Полноэкранный выбор области мышью.
        let mut picked: Option<Region> = None;
        let mut cancel = false;
        if let Picker::Selecting { tex, size, start, cur } = &mut self.picker {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("region_picker"),
                egui::ViewportBuilder::default()
                    .with_title("Выбор области")
                    .with_fullscreen(true)
                    .with_decorations(false)
                    .with_always_on_top(),
                |ctx, _| {
                    egui::CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| {
                        let rect = ui.max_rect();
                        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                        let painter = ui.painter();
                        painter.image(
                            tex.id(),
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(110));

                        if resp.drag_started() {
                            *start = resp.interact_pointer_pos();
                        }
                        if resp.dragged() {
                            *cur = resp.interact_pointer_pos();
                        }

                        if let (Some(a), Some(b)) = (*start, *cur) {
                            let sel = egui::Rect::from_two_pos(a, b);
                            painter.image(
                                tex.id(),
                                sel,
                                egui::Rect::from_min_max(
                                    egui::pos2(
                                        (sel.min.x - rect.min.x) / rect.width(),
                                        (sel.min.y - rect.min.y) / rect.height(),
                                    ),
                                    egui::pos2(
                                        (sel.max.x - rect.min.x) / rect.width(),
                                        (sel.max.y - rect.min.y) / rect.height(),
                                    ),
                                ),
                                Color32::WHITE,
                            );
                            painter.rect_stroke(
                                sel,
                                0.0,
                                Stroke::new(2.0_f32, ACCENT),
                                egui::StrokeKind::Outside,
                            );

                            if resp.drag_stopped() {
                                let sx = size[0] as f32 / rect.width();
                                let sy = size[1] as f32 / rect.height();
                                let x = ((sel.min.x - rect.min.x) * sx).max(0.0) as u32;
                                let y = ((sel.min.y - rect.min.y) * sy).max(0.0) as u32;
                                let w = (sel.width() * sx).max(1.0) as u32;
                                let h = (sel.height() * sy).max(1.0) as u32;
                                if w >= 20 && h >= 10 {
                                    picked = Some(Region { x, y, w, h });
                                } else {
                                    *start = None;
                                    *cur = None;
                                }
                            }
                        }

                        painter.text(
                            egui::pos2(rect.center().x, rect.min.y + 36.0),
                            egui::Align2::CENTER_CENTER,
                            "Выдели мышью область с субтитрами · Esc — отмена",
                            egui::FontId::proportional(22.0),
                            Color32::WHITE,
                        );
                    });
                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel = true;
                    }
                },
            );
        }
        if let Some(r) = picked {
            sh.cfg.region = r;
            self.picker = Picker::Idle;
        } else if cancel {
            self.picker = Picker::Idle;
        }

        // Автосохранение настроек с небольшой задержкой.
        let text = sh.cfg.to_toml();
        if text != self.saved_text {
            match self.dirty_since {
                None => self.dirty_since = Some(Instant::now()),
                Some(t) if t.elapsed() > Duration::from_millis(600) => {
                    Config::save_text(&text);
                    self.saved_text = text;
                    self.dirty_since = None;
                }
                _ => {}
            }
        } else {
            self.dirty_since = None;
        }
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        egui::TopBottomPanel::top("top")
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(20, 14)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Voice").size(26.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new("озвучка субтитров с экрана").size(13.0).color(MUTED));
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (label, fill) = if sh.running { ("Стоп", WARN) } else { ("Старт", ACCENT) };
                        let btn = egui::Button::new(RichText::new(label).size(17.0).strong().color(Color32::WHITE))
                            .fill(fill)
                            .corner_radius(CornerRadius::same(10))
                            .min_size(Vec2::new(130.0, 40.0));
                        if ui.add(btn).clicked() {
                            sh.running = !sh.running;
                            if sh.running {
                                sh.error = None;
                            }
                        }

                        ui.add_space(18.0);
                        let (text, color) = status(sh);
                        ui.label(RichText::new(text).size(15.0).color(color));
                        let (dot, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        let t = ctx.input(|i| i.time) as f32;
                        let pulse = if sh.running { 0.55 + 0.45 * (t * 4.0).sin().abs() } else { 0.5 };
                        ui.painter().circle_filled(dot.center(), 6.0, color.gamma_multiply(pulse));
                    });
                });
            });
    }

    fn main_panel(&mut self, ctx: &egui::Context, sh: &Shared) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin::same(20)))
            .show(ctx, |ui| {
                if let Some(err) = &sh.error {
                    Frame::new()
                        .fill(Color32::from_rgb(60, 24, 30))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(RichText::new(err).color(Color32::from_rgb(255, 170, 170)));
                        });
                    ui.add_space(12.0);
                }

                card(ui, "Сейчас", |ui| {
                    match sh.history.last() {
                        Some(e) => {
                            if let Some(name) = &e.speaker {
                                chip(ui, name);
                                ui.add_space(4.0);
                            }
                            ui.label(RichText::new(&e.text).size(26.0).color(Color32::WHITE));
                        }
                        None => {
                            ui.label(
                                RichText::new("Жду субтитры… Нажми «Старт» и включи субтитры в игре")
                                    .size(18.0)
                                    .color(MUTED),
                            );
                        }
                    }
                    ui.add_space(6.0);
                });

                ui.add_space(14.0);
                card(ui, "История", |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        if sh.history.is_empty() {
                            ui.label(RichText::new("Пока пусто").color(MUTED));
                        }
                        for e in sh.history.iter().rev() {
                            let secs = e.at.duration_since(self.started).as_secs();
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!("{:02}:{:02}", secs / 60, secs % 60))
                                        .monospace()
                                        .color(MUTED),
                                );
                                if let Some(name) = &e.speaker {
                                    chip(ui, name);
                                }
                                ui.label(RichText::new(&e.text).size(15.0));
                            });
                            ui.add_space(2.0);
                        }
                    });
                });
            });
    }

    fn settings_panel(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        egui::SidePanel::right("settings")
            .exact_width(360.0)
            .resizable(false)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(16)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // ----- Область -----
                    card(ui, "Область субтитров", |ui| {
                        ui.label(RichText::new("Окно для захвата (часть названия)").size(12.0).color(MUTED));
                        ui.add(
                            egui::TextEdit::singleline(&mut sh.cfg.capture_window)
                                .desired_width(f32::INFINITY)
                                .hint_text("chiaki — или пусто, чтобы снимать весь монитор"),
                        );
                        if sh.monitors > 1 && sh.cfg.capture_window.trim().is_empty() {
                            egui::ComboBox::from_id_salt("monitor")
                                .selected_text(format!("Монитор {}", sh.cfg.monitor + 1))
                                .show_ui(ui, |ui| {
                                    for i in 0..sh.monitors {
                                        ui.selectable_value(&mut sh.cfg.monitor, i, format!("Монитор {}", i + 1));
                                    }
                                });
                        }

                        if let Some(tex) = &self.preview_tex {
                            let w = ui.available_width();
                            let s = tex.size_vec2();
                            let h = (w * s.y / s.x.max(1.0)).clamp(30.0, 140.0);
                            ui.add(egui::Image::new((tex.id(), Vec2::new(w, h))).corner_radius(CornerRadius::same(8)));
                        }

                        let r = &mut sh.cfg.region;
                        egui::Grid::new("region").num_columns(4).show(ui, |ui| {
                            ui.label("X");
                            ui.add(egui::DragValue::new(&mut r.x).range(0..=10000).speed(2));
                            ui.label("Y");
                            ui.add(egui::DragValue::new(&mut r.y).range(0..=10000).speed(2));
                            ui.end_row();
                            ui.label("Ш");
                            ui.add(egui::DragValue::new(&mut r.w).range(20..=10000).speed(2));
                            ui.label("В");
                            ui.add(egui::DragValue::new(&mut r.h).range(10..=10000).speed(2));
                            ui.end_row();
                        });

                        let label = match &self.picker {
                            Picker::Waiting { until } => {
                                let left = until.saturating_duration_since(Instant::now()).as_secs() + 1;
                                format!("Переключись на игру… {left}")
                            }
                            _ => "Выбрать мышью на экране".to_string(),
                        };
                        let busy = !matches!(self.picker, Picker::Idle);
                        if ui.add_enabled(!busy, egui::Button::new(label).min_size(Vec2::new(ui.available_width(), 32.0))).clicked() {
                            let (m, w) = (sh.cfg.monitor, sh.cfg.capture_window.clone());
                            self.start_picker(ctx, m, w);
                        }
                        ui.label(RichText::new("Нажми, переключись в игру с видимыми субтитрами — через 4 с экран снимется.").size(12.0).color(MUTED));

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Частота чтения");
                            ui.add(egui::Slider::new(&mut sh.cfg.interval_ms, 100..=1000).suffix(" мс"));
                        });
                    });

                    ui.add_space(12.0);

                    // ----- Голос -----
                    card(ui, "Голос", |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut sh.cfg.speech.engine, Engine::Windows, "Windows");
                            ui.selectable_value(&mut sh.cfg.speech.engine, Engine::Piper, "Piper");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Скорость");
                            ui.add(egui::Slider::new(&mut sh.cfg.speech.speed, 0.6..=1.8).fixed_decimals(2));
                        });

                        let engine = sh.cfg.speech.engine;
                        let voices = match engine {
                            Engine::Windows => sh.voices.clone(),
                            Engine::Piper => sh.piper_models.clone(),
                        };
                        ui.horizontal(|ui| {
                            ui.label("Основной");
                            voice_picker(ui, "default_voice", engine, &voices, &mut sh.cfg.speech.default_voice);
                        });

                        if engine == Engine::Piper {
                            if let Some(text) = sh.download_status.clone() {
                                ui.label(RichText::new(text).size(12.0).color(OK));
                            }
                            let label = if sh.piper_models.is_empty() {
                                "Скачать Piper и русские голоса (~150 МБ)"
                            } else {
                                "Докачать недостающие голоса"
                            };
                            if sh.piper_models.is_empty() {
                                ui.label(RichText::new("Голоса Piper звучат намного естественнее, чем голоса Windows. Скачай их одной кнопкой.").size(12.0).color(MUTED));
                            }
                            if ui.add_enabled(!sh.downloading, egui::Button::new(label)).clicked() {
                                let arc = self.shared.clone();
                                crate::piper::start_install(sh, arc, ctx.clone());
                            }
                        } else if voices.is_empty() {
                            ui.label(RichText::new("Голоса Windows не найдены. Добавь русский голос: Параметры → Специальные возможности → Диктор.").size(12.0).color(WARN));
                        }

                        if ui.button("Проверить голос").clicked() {
                            let _ = self.tx.send(Utterance {
                                text: "Привет! Так будут звучать субтитры из игры.".into(),
                                voice: String::new(),
                                pitch: 1.0,
                            });
                        }
                    });

                    ui.add_space(12.0);

                    // ----- Говорящие -----
                    card(ui, "Голоса героев", |ui| {
                        ui.label(RichText::new("Новые герои из субтитров «Имя: текст» появляются здесь сами с разным тоном. Голос и тон можно поправить.").size(12.0).color(MUTED));
                        let engine = sh.cfg.speech.engine;
                        let voices = match engine {
                            Engine::Windows => sh.voices.clone(),
                            Engine::Piper => sh.piper_models.clone(),
                        };
                        let mut remove = None;
                        for (i, rule) in sh.cfg.rules.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut rule.name).desired_width(90.0).hint_text("Имя"));
                                voice_picker(ui, ("rule_voice", i), engine, &voices, &mut rule.voice);
                                ui.add(egui::DragValue::new(&mut rule.pitch).range(0.6..=1.5).speed(0.01).fixed_decimals(2).prefix("тон "));
                                if ui.small_button("×").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            sh.cfg.rules.remove(i);
                        }
                        if ui.button("+ Добавить героя").clicked() {
                            sh.cfg.rules.push(Rule { name: String::new(), voice: String::new(), pitch: 1.0 });
                        }
                    });
                });
            });
    }
}

fn voice_picker(ui: &mut egui::Ui, id: impl std::hash::Hash, engine: Engine, voices: &[String], value: &mut String) {
    let name = |v: &str| match engine {
        Engine::Piper => crate::piper::short_name(v),
        Engine::Windows => v.to_string(),
    };
    let shown = if value.is_empty() { "по умолчанию".to_string() } else { name(value) };
    egui::ComboBox::from_id_salt(id).width(150.0).selected_text(shown).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), "по умолчанию");
        for v in voices {
            ui.selectable_value(value, v.clone(), name(v));
        }
    });
}

fn status(sh: &Shared) -> (&'static str, Color32) {
    if !sh.running {
        return ("Остановлено", MUTED);
    }
    match sh.last_seen {
        Some(t) if t.elapsed() < Duration::from_millis(1500) => ("Читаю субтитры", OK),
        _ => ("Слушаю", ACCENT),
    }
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(13.0).strong().color(MUTED));
            ui.add_space(6.0);
            add(ui);
        });
}

fn chip(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(ACCENT.gamma_multiply(0.35))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(13.0).strong().color(Color32::WHITE));
        });
}

fn apply_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(16, 18, 27);
    v.selection.bg_fill = ACCENT;
    v.widgets.inactive.bg_fill = Color32::from_rgb(40, 45, 62);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 45, 62);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(54, 60, 82);
    v.widgets.hovered.bg_fill = Color32::from_rgb(54, 60, 82);
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.noninteractive,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
    });
}
