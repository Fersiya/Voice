use crate::capture::{crop, Capturer};
use crate::ocr::Ocr;
use crate::state::{Entry, Shared};
use crate::tts::Utterance;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Поток распознавания: всегда обновляет превью области, а когда включён «Старт» —
/// читает субтитры и отправляет новые строки на озвучку.
pub fn run(shared: Arc<Mutex<Shared>>, tx: Sender<Utterance>, ctx: eframe::egui::Context) {
    let mut capturer: Option<((usize, String), Capturer)> = None;
    let mut ocr: Option<(String, Ocr)> = None;

    let mut candidate = String::new();
    let mut stable = 0u8;
    let mut last_spoken = String::new();

    loop {
        let (cfg, running) = {
            let s = shared.lock().unwrap();
            (s.cfg.clone(), s.running)
        };
        std::thread::sleep(Duration::from_millis(cfg.interval_ms.clamp(100, 2000)));

        // Захват экрана (пересоздаём, если сменился монитор).
        let key = (cfg.monitor, cfg.capture_window.clone());
        if capturer.as_ref().map(|(k, _)| k) != Some(&key) {
            match Capturer::new(cfg.monitor, &cfg.capture_window) {
                Ok(c) => capturer = Some((key, c)),
                Err(e) => {
                    set_error(&shared, format!("{e:#}"));
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
        }
        let full = match capturer.as_ref().unwrap().1.grab() {
            Ok(i) => i,
            Err(e) => {
                set_error(&shared, format!("{e:#}"));
                capturer = None;
                continue;
            }
        };
        let Some(img) = crop(&full, cfg.region) else {
            set_error(&shared, "Область за пределами экрана".into());
            continue;
        };

        {
            let mut s = shared.lock().unwrap();
            s.preview = Some(img.clone());
            s.preview_gen += 1;
            if !s.error.as_deref().is_some_and(|e| e.starts_with("Озвучка")) {
                s.error = None;
            }
        }
        ctx.request_repaint();

        if !running {
            candidate.clear();
            stable = 0;
            continue;
        }

        // OCR (создаём при первом запуске или смене языка).
        if ocr.as_ref().map(|(l, _)| l.as_str()) != Some(cfg.ocr_lang.as_str()) {
            match Ocr::new(&cfg.ocr_lang) {
                Ok(o) => ocr = Some((cfg.ocr_lang.clone(), o)),
                Err(e) => {
                    let mut s = shared.lock().unwrap();
                    s.error = Some(format!("{e:#}"));
                    s.running = false;
                    continue;
                }
            }
        }
        let raw = match ocr.as_ref().unwrap().1.read(&img) {
            Ok(t) => t,
            Err(e) => {
                set_error(&shared, format!("OCR: {e:#}"));
                continue;
            }
        };

        let text = normalize(&raw);
        if !looks_like_text(&text) {
            candidate.clear();
            stable = 0;
            continue;
        }
        shared.lock().unwrap().last_seen = Some(Instant::now());

        // Говорим, когда одна и та же строка прочиталась дважды подряд:
        // так не озвучиваются недопоявившиеся или угасающие субтитры.
        if text == candidate {
            stable = stable.saturating_add(1);
        } else {
            candidate = text.clone();
            stable = 1;
        }
        if stable != 2 || is_same(&text, &last_spoken) {
            continue;
        }
        last_spoken = text.clone();

        let (speaker, line) = split_speaker(&text);
        let (voice, pitch) = match speaker {
            Some(n) => voice_for(&shared, n),
            None => (String::new(), 1.0),
        };

        {
            let mut s = shared.lock().unwrap();
            s.history.push(Entry {
                at: Instant::now(),
                speaker: speaker.map(str::to_string),
                text: line.to_string(),
            });
            if s.history.len() > 200 {
                s.history.remove(0);
            }
        }
        let _ = tx.send(Utterance { text: line.to_string(), voice, pitch });
        ctx.request_repaint();
    }
}

fn set_error(shared: &Arc<Mutex<Shared>>, msg: String) {
    shared.lock().unwrap().error = Some(msg);
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Отсекает шум OCR: слишком коротко или почти нет букв.
fn looks_like_text(s: &str) -> bool {
    let total = s.chars().count();
    if total < 3 {
        return false;
    }
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    letters * 100 / total >= 50
}

/// Та же строка, если совпала или одна содержит другую (угасание, края OCR).
fn is_same(a: &str, b: &str) -> bool {
    a == b || (!b.is_empty() && (a.contains(b) || b.contains(a)))
}

/// Формат «Имя: текст». Имя — одно слово в начале строки.
fn split_speaker(text: &str) -> (Option<&str>, &str) {
    if let Some((name, rest)) = text.split_once(':') {
        let name = name.trim();
        if !name.is_empty() && name.chars().count() <= 20 && !name.contains(' ') {
            return (Some(name), rest.trim());
        }
    }
    (None, text)
}

/// Тоны для новых героев: чередуем ниже/выше, чтобы голоса отличались.
const PITCHES: [f32; 8] = [1.0, 0.82, 1.18, 0.9, 1.3, 0.75, 1.08, 1.22];

/// Находит правило героя по имени или создаёт новое с отдельным тоном
/// (и другим голосом Windows, если их несколько).
fn voice_for(shared: &Arc<Mutex<Shared>>, name: &str) -> (String, f32) {
    let mut s = shared.lock().unwrap();
    let key = name.trim().to_lowercase();
    if let Some(r) = s.cfg.rules.iter().find(|r| r.name.trim().to_lowercase() == key) {
        return (r.voice.clone(), r.pitch);
    }

    let idx = s.cfg.rules.len();
    let pitch = PITCHES[idx % PITCHES.len()];
    let list = match s.cfg.speech.engine {
        crate::config::Engine::Windows => &s.voices,
        crate::config::Engine::Piper => &s.piper_models,
    };
    let voice = if list.len() > 1 { list[idx % list.len()].clone() } else { String::new() };
    s.cfg.rules.push(crate::config::Rule { name: name.trim().to_string(), voice: voice.clone(), pitch });
    (voice, pitch)
}
