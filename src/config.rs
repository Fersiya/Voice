use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub monitor: usize,
    pub region: Region,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default = "default_lang")]
    pub ocr_lang: String,
    pub piper: Piper,
    #[serde(default)]
    pub voices: HashMap<String, u32>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Deserialize)]
pub struct Piper {
    pub exe: String,
    pub model: String,
    #[serde(default = "default_length_scale")]
    pub length_scale: f32,
    #[serde(default)]
    pub default_speaker: u32,
}

fn default_interval() -> u64 {
    250
}
fn default_lang() -> String {
    "ru".into()
}
fn default_length_scale() -> f32 {
    1.0
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("не удалось прочитать {}", path.display()))?;
        toml::from_str(&text).context("ошибка в config.toml")
    }
}
