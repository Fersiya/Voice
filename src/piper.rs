use crate::state::Shared;
use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const PIPER_ZIP: &str =
    "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip";
const VOICES_BASE: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main/ru/ru_RU";
/// Русские голоса Piper: мужские denis, dmitri, ruslan и женский irina.
const VOICES: [&str; 4] = ["denis", "dmitri", "ruslan", "irina"];

/// Папка рядом с exe, где лежат piper.exe и голоса.
pub fn dir() -> PathBuf {
    crate::config::Config::base_dir().join("piper")
}

pub fn exe() -> PathBuf {
    dir().join("piper.exe")
}

/// Установленные голоса: имена файлов .onnx без расширения.
pub fn scan() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ok = p.extension().is_some_and(|x| x == "onnx") && p.with_extension("onnx.json").exists();
            ok.then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
        })
        .collect();
    out.sort();
    out
}

pub fn model_path(stem: &str) -> PathBuf {
    dir().join(format!("{stem}.onnx"))
}

/// Короткое имя для списка: ru_RU-denis-medium → denis.
pub fn short_name(stem: &str) -> String {
    stem.split('-').nth(1).unwrap_or(stem).to_string()
}

/// Скачивает Piper и русские голоса в фоне, прогресс пишет в Shared.
pub fn start_install(sh: &mut Shared, shared: Arc<Mutex<Shared>>, ctx: eframe::egui::Context) {
    if sh.downloading {
        return;
    }
    sh.downloading = true;
    sh.download_status = Some("Начинаю загрузку…".into());

    std::thread::spawn(move || {
        let status = |text: String| {
            shared.lock().unwrap().download_status = Some(text);
            ctx.request_repaint();
        };
        let result = install(&status);

        let mut s = shared.lock().unwrap();
        s.downloading = false;
        s.piper_models = scan();
        match result {
            Ok(()) => s.download_status = Some("Готово: голоса установлены".into()),
            Err(e) => {
                s.download_status = None;
                s.error = Some(format!("Загрузка Piper: {e:#}"));
            }
        }
        ctx.request_repaint();
    });
}

fn install(status: &dyn Fn(String)) -> Result<()> {
    let base = crate::config::Config::base_dir();
    std::fs::create_dir_all(dir())?;

    if !exe().exists() {
        status("Скачиваю Piper (около 20 МБ)…".into());
        let bytes = download(PIPER_ZIP)?;
        status("Распаковываю Piper…".into());
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("архив Piper повреждён")?;
        zip.extract(&base).context("не удалось распаковать Piper")?;
        if !exe().exists() {
            return Err(anyhow!("после распаковки нет piper/piper.exe"));
        }
    }

    for (i, name) in VOICES.iter().enumerate() {
        let stem = format!("ru_RU-{name}-medium");
        let onnx = model_path(&stem);
        if onnx.exists() && onnx.with_extension("onnx.json").exists() {
            continue;
        }
        status(format!("Голос {} из {}: {name}…", i + 1, VOICES.len()));
        let url = format!("{VOICES_BASE}/{name}/medium/{stem}.onnx");
        save(&url, &onnx)?;
        save(&format!("{url}.json"), &onnx.with_extension("onnx.json"))?;
    }
    Ok(())
}

fn download(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url).call().map_err(|e| anyhow!("{url}: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

/// Сначала во временный файл, чтобы оборванная загрузка не оставила битую модель.
fn save(url: &str, dest: &Path) -> Result<()> {
    let bytes = download(url)?;
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}
