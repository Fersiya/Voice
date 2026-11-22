use crate::config::Config;
use image::RgbaImage;
use std::time::Instant;

pub struct Entry {
    pub at: Instant,
    pub speaker: Option<String>,
    pub text: String,
}

/// Общее состояние: UI, поток распознавания и поток озвучки.
pub struct Shared {
    pub cfg: Config,
    pub running: bool,
    pub error: Option<String>,
    pub preview: Option<RgbaImage>,
    pub preview_gen: u64,
    pub history: Vec<Entry>,
    pub voices: Vec<String>,
    pub monitors: usize,
    pub last_seen: Option<Instant>,
}

impl Shared {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            running: false,
            error: None,
            preview: None,
            preview_gen: 0,
            history: Vec::new(),
            voices: Vec::new(),
            monitors: 1,
            last_seen: None,
        }
    }
}
