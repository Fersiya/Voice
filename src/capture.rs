use crate::config::Region;
use anyhow::{anyhow, Result};
use image::RgbaImage;

pub struct Capturer {
    #[cfg(windows)]
    monitor: xcap::Monitor,
}

#[cfg(windows)]
impl Capturer {
    pub fn new(index: usize) -> Result<Self> {
        let mut monitors = xcap::Monitor::all().map_err(|e| anyhow!("мониторы: {e}"))?;
        if index >= monitors.len() {
            return Err(anyhow!("монитор {} не найден", index + 1));
        }
        Ok(Self { monitor: monitors.swap_remove(index) })
    }

    /// Весь монитор целиком.
    pub fn grab(&self) -> Result<RgbaImage> {
        let img = self
            .monitor
            .capture_image()
            .map_err(|e| anyhow!("захват экрана: {e}"))?;
        // Пересобираем из сырых байт, чтобы не зависеть от версии image внутри xcap.
        RgbaImage::from_raw(img.width(), img.height(), img.into_raw())
            .ok_or_else(|| anyhow!("не удалось собрать изображение"))
    }
}

#[cfg(not(windows))]
impl Capturer {
    pub fn new(_index: usize) -> Result<Self> {
        Err(anyhow!("захват экрана работает только в Windows"))
    }

    pub fn grab(&self) -> Result<RgbaImage> {
        Err(anyhow!("захват экрана работает только в Windows"))
    }
}

pub fn monitor_count() -> usize {
    #[cfg(windows)]
    {
        xcap::Monitor::all().map(|m| m.len()).unwrap_or(1).max(1)
    }
    #[cfg(not(windows))]
    {
        1
    }
}

/// Вырезает область, обрезая её по границам экрана.
pub fn crop(full: &RgbaImage, r: Region) -> Option<RgbaImage> {
    let (fw, fh) = (full.width(), full.height());
    if r.x >= fw || r.y >= fh || r.w == 0 || r.h == 0 {
        return None;
    }
    let w = r.w.min(fw - r.x);
    let h = r.h.min(fh - r.y);
    Some(image::imageops::crop_imm(full, r.x, r.y, w, h).to_image())
}
