use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub monitor: usize,
    /// Часть названия окна для захвата (например, "chiaki"). Пусто — захват всего монитора.
    pub capture_window: String,
    pub region: Region,
    pub interval_ms: u64,
    pub ocr_lang: String,
    pub speech: Speech,
    /// Голос по имени говорящего («Имя: текст» в субтитрах).
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// Голоса Windows: работает сразу, ничего скачивать не нужно.
    Windows,
    /// Piper: нейросетевые голоса, нужны piper.exe и модель.
    Piper,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Speech {
    pub engine: Engine,
    /// 1.0 — обычная скорость.
    pub speed: f32,
    /// Windows: имя голоса. Piper: имя модели. Пусто — по умолчанию.
    pub default_voice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub voice: String,
    /// Тон голоса: 1.0 — обычный, меньше — ниже, больше — выше.
    #[serde(default = "one")]
    pub pitch: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            monitor: 0,
            capture_window: "chiaki".into(),
            region: Region { x: 560, y: 930, w: 800, h: 90 },
            interval_ms: 250,
            ocr_lang: "ru".into(),
            speech: Speech::default(),
            rules: Vec::new(),
        }
    }
}

impl Default for Speech {
    fn default() -> Self {
        Self {
            engine: Engine::Windows,
            speed: 1.0,
            default_voice: String::new(),
        }
    }
}

impl Config {
    /// Папка с exe: рядом лежат config.toml и piper/.
    pub fn base_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn path() -> PathBuf {
        Self::base_dir().join("config.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn save_text(text: &str) {
        let _ = std::fs::write(Self::path(), text);
    }
}
