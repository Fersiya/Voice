use crate::config::Region;
use anyhow::Result;
use image::RgbaImage;

/// Захват либо окна по части названия (например, Chiaki), либо всего монитора.
/// Область субтитров считается от левого верхнего угла того, что снимается.
pub struct Capturer {
    #[cfg_attr(not(windows), allow(dead_code))]
    monitor: usize,
    #[cfg_attr(not(windows), allow(dead_code))]
    window: String,
}

#[cfg(windows)]
impl Capturer {
    pub fn new(monitor: usize, window: &str) -> Result<Self> {
        Ok(Self { monitor, window: window.trim().to_lowercase() })
    }

    pub fn grab(&self) -> Result<RgbaImage> {
        use anyhow::anyhow;

        let raw = if self.window.is_empty() {
            let mut monitors = xcap::Monitor::all().map_err(|e| anyhow!("мониторы: {e}"))?;
            if self.monitor >= monitors.len() {
                return Err(anyhow!("монитор {} не найден", self.monitor + 1));
            }
            monitors
                .swap_remove(self.monitor)
                .capture_image()
                .map_err(|e| anyhow!("захват экрана: {e}"))?
        } else {
            let windows = xcap::Window::all().map_err(|e| anyhow!("окна: {e}"))?;
            // Подходящее окно — по названию окна или программы; берём самое большое.
            let win = windows
                .iter()
                .filter(|w| !w.is_minimized().unwrap_or(false))
                .filter(|w| {
                    let t = w.title().unwrap_or_default().to_lowercase();
                    let a = w.app_name().unwrap_or_default().to_lowercase();
                    t.contains(&self.window) || a.contains(&self.window)
                })
                .max_by_key(|w| w.width().unwrap_or(0) as u64 * w.height().unwrap_or(0) as u64)
                .ok_or_else(|| {
                    anyhow!(
                        "Окно «{}» не найдено или свёрнуто. Запусти Chiaki или очисти поле, чтобы снимать весь монитор",
                        self.window
                    )
                })?;
            win.capture_image().map_err(|e| anyhow!("захват окна: {e}"))?
        };

        // Пересобираем из сырых байт, чтобы не зависеть от версии image внутри xcap.
        RgbaImage::from_raw(raw.width(), raw.height(), raw.into_raw())
            .ok_or_else(|| anyhow!("не удалось собрать изображение"))
    }
}

#[cfg(not(windows))]
impl Capturer {
    pub fn new(monitor: usize, window: &str) -> Result<Self> {
        Ok(Self { monitor, window: window.to_string() })
    }

    pub fn grab(&self) -> Result<RgbaImage> {
        Err(anyhow::anyhow!("захват экрана работает только в Windows"))
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

/// Вырезает область, обрезая её по границам кадра.
pub fn crop(full: &RgbaImage, r: Region) -> Option<RgbaImage> {
    let (fw, fh) = (full.width(), full.height());
    if r.x >= fw || r.y >= fh || r.w == 0 || r.h == 0 {
        return None;
    }
    let w = r.w.min(fw - r.x);
    let h = r.h.min(fh - r.y);
    Some(image::imageops::crop_imm(full, r.x, r.y, w, h).to_image())
}
