mod capture;
mod config;
mod ocr;
mod tts;

use anyhow::Result;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

fn main() -> Result<()> {
    let cfg = config::Config::load(Path::new("config.toml"))?;
    let capturer = capture::Capturer::new(cfg.monitor, cfg.region)?;

    // `voice --shot` сохраняет снимок области в debug.png: так удобно подобрать координаты.
    if std::env::args().any(|a| a == "--shot") {
        capturer.grab()?.save("debug.png")?;
        println!("Сохранено в debug.png");
        return Ok(());
    }

    let ocr = ocr::Ocr::new(&cfg.ocr_lang)?;

    let (tx, rx) = mpsc::channel();
    let piper_cfg = config::Piper {
        exe: cfg.piper.exe.clone(),
        model: cfg.piper.model.clone(),
        length_scale: cfg.piper.length_scale,
        default_speaker: cfg.piper.default_speaker,
    };
    std::thread::spawn(move || {
        if let Err(e) = tts::run(piper_cfg, rx) {
            eprintln!("Поток озвучки остановлен: {e:#}");
        }
    });

    println!("Слежу за областью {:?}. Ctrl+C — выход.", cfg.region);

    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(cfg.interval_ms));

        let img = match capturer.grab() {
            Ok(i) => i,
            Err(e) => {
                eprintln!("{e:#}");
                continue;
            }
        };
        let raw = match ocr.read(&img) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("OCR: {e:#}");
                continue;
            }
        };

        let text = normalize(&raw);
        if text.chars().count() < 3 || is_same(&text, &last) {
            continue;
        }
        last = text.clone();

        let (speaker_name, line) = split_speaker(&text);
        let speaker = speaker_name
            .and_then(|n| cfg.voices.get(n).copied())
            .unwrap_or(cfg.piper.default_speaker);

        println!("[{speaker}] {line}");
        let _ = tx.send(tts::Utterance {
            text: line.to_string(),
            speaker,
        });
    }
}

/// Схлопывает переносы строк и лишние пробелы.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Субтитр считается тем же, если текст совпал или один содержит другой
/// (например, строка появляется постепенно или OCR чуть сбился на краях).
fn is_same(a: &str, b: &str) -> bool {
    a == b || (!b.is_empty() && (a.contains(b) || b.contains(a)))
}

/// Формат «Имя: текст». Имя берём только если двоеточие в начале строки.
fn split_speaker(text: &str) -> (Option<&str>, &str) {
    if let Some((name, rest)) = text.split_once(':') {
        let name = name.trim();
        if !name.is_empty() && name.chars().count() <= 20 && !name.contains(' ') {
            return (Some(name), rest.trim());
        }
    }
    (None, text)
}
