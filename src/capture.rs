use crate::config::Region;
use anyhow::{anyhow, Result};
use image::RgbaImage;
use xcap::Monitor;

pub struct Capturer {
    monitor: Monitor,
    region: Region,
}

impl Capturer {
    pub fn new(index: usize, region: Region) -> Result<Self> {
        let mut monitors = Monitor::all().map_err(|e| anyhow!("мониторы: {e}"))?;
        if index >= monitors.len() {
            return Err(anyhow!(
                "монитор {index} не найден, всего мониторов: {}",
                monitors.len()
            ));
        }
        Ok(Self {
            monitor: monitors.swap_remove(index),
            region,
        })
    }

    /// Снимок области субтитров.
    pub fn grab(&self) -> Result<RgbaImage> {
        let full = self
            .monitor
            .capture_image()
            .map_err(|e| anyhow!("захват экрана: {e}"))?;
        let (fw, fh) = (full.width(), full.height());
        let r = self.region;
        if r.x >= fw || r.y >= fh {
            return Err(anyhow!("область за пределами экрана {fw}x{fh}"));
        }
        let w = r.w.min(fw - r.x);
        let h = r.h.min(fh - r.y);

        // Копируем пиксели вручную, чтобы не зависеть от версии image внутри xcap.
        let raw = full.as_raw();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let start = (((r.y + row) * fw + r.x) * 4) as usize;
            out.extend_from_slice(&raw[start..start + (w * 4) as usize]);
        }
        RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("не удалось собрать изображение"))
    }
}
