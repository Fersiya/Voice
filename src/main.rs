#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod ocr;
mod piper;
mod state;
mod tts;
mod worker;

use std::sync::{mpsc, Arc, Mutex};

fn main() -> eframe::Result<()> {
    let cfg = config::Config::load();
    let mut shared = state::Shared::new(cfg);
    shared.monitors = capture::monitor_count();
    shared.piper_models = piper::scan();
    let shared = Arc::new(Mutex::new(shared));

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Voice — озвучка субтитров")
            .with_inner_size([1040.0, 680.0])
            .with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Voice",
        options,
        Box::new(move |cc| {
            let (tx, rx) = mpsc::channel();

            let s = shared.clone();
            std::thread::spawn(move || tts::run(s, rx));

            let s = shared.clone();
            let t = tx.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || worker::run(s, t, ctx));

            Ok(Box::new(app::App::new(&cc.egui_ctx, shared, tx)))
        }),
    )
}
