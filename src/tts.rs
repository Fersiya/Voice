use crate::state::Shared;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

pub struct Utterance {
    pub text: String,
    /// Windows: имя голоса. Piper: номер спикера. Пусто — по умолчанию.
    pub voice: String,
    /// Тон: 1.0 — обычный.
    pub pitch: f32,
}

/// Поток озвучки. Новая реплика прерывает текущую, накопившиеся пропускаются.
pub fn run(shared: Arc<Mutex<Shared>>, rx: Receiver<Utterance>) {
    imp::run(shared, rx)
}

#[cfg(windows)]
mod imp {
    use super::Utterance;
    use crate::config::{Engine, Speech};
    use crate::state::Shared;
    use anyhow::{anyhow, Context, Result};
    use rodio::buffer::SamplesBuffer;
    use rodio::{OutputStream, OutputStreamHandle, Sink};
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::Receiver;
    use std::sync::{Arc, Mutex};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    pub fn run(shared: Arc<Mutex<Shared>>, rx: Receiver<Utterance>) {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }

        let mut sys = tts::Tts::default().ok();
        if let Some(t) = &sys {
            if let Ok(all) = t.voices() {
                let ru: Vec<_> = all
                    .iter()
                    .filter(|v| v.language().to_string().to_lowercase().starts_with("ru"))
                    .collect();
                let pick: Vec<_> = if ru.is_empty() { all.iter().collect() } else { ru };
                shared.lock().unwrap().voices = pick.iter().map(|v| v.name()).collect();
            }
        }

        let mut out: Option<(OutputStream, OutputStreamHandle)> = None;
        let mut sink: Option<Sink> = None;

        while let Ok(mut u) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                u = newer;
            }
            let speech = shared.lock().unwrap().cfg.speech.clone();

            let res = match speech.engine {
                Engine::Windows => speak_windows(&mut sys, &speech, &u),
                Engine::Piper => speak_piper(&mut out, &mut sink, &speech, &u),
            };
            if let Err(e) = res {
                shared.lock().unwrap().error = Some(format!("Озвучка: {e:#}"));
            }
        }
    }

    fn speak_windows(sys: &mut Option<tts::Tts>, speech: &Speech, u: &Utterance) -> Result<()> {
        let t = sys.as_mut().ok_or_else(|| anyhow!("системный синтезатор речи недоступен"))?;

        let voice = if u.voice.is_empty() { &speech.default_voice } else { &u.voice };
        if !voice.is_empty() {
            if let Ok(all) = t.voices() {
                if let Some(v) = all.iter().find(|v| &v.name() == voice) {
                    let _ = t.set_voice(v);
                }
            }
        }

        let (min, normal, max) = (t.min_rate(), t.normal_rate(), t.max_rate());
        let s = speech.speed.clamp(0.5, 2.0);
        let rate = if s >= 1.0 {
            normal + (max - normal) * (s - 1.0)
        } else {
            normal - (normal - min) * ((1.0 - s) / 0.5)
        };
        let _ = t.set_rate(rate);

        let p = u.pitch.clamp(0.5, 2.0);
        let (pmin, pnorm, pmax) = (t.min_pitch(), t.normal_pitch(), t.max_pitch());
        let _ = t.set_pitch((pnorm * p).clamp(pmin, pmax));

        t.speak(&u.text, true).map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }

    fn speak_piper(
        out: &mut Option<(OutputStream, OutputStreamHandle)>,
        sink: &mut Option<Sink>,
        speech: &Speech,
        u: &Utterance,
    ) -> Result<()> {
        if let Some(s) = sink.take() {
            s.stop();
        }

        // Модель героя → основная → первая установленная.
        let installed = crate::piper::scan();
        let wanted = if u.voice.is_empty() { &speech.default_voice } else { &u.voice };
        let stem = if installed.contains(wanted) {
            wanted.clone()
        } else {
            installed
                .first()
                .cloned()
                .ok_or_else(|| anyhow!("голоса Piper не установлены: нажми «Скачать Piper и голоса»"))?
        };
        if !crate::piper::exe().exists() {
            return Err(anyhow!("piper.exe не найден: нажми «Скачать Piper и голоса»"));
        }
        // Тон меняем скоростью воспроизведения (она двигает и тон, и темп),
        // темп возвращаем обратно через length_scale.
        let pitch = u.pitch.clamp(0.7, 1.4);
        let length_scale = pitch / speech.speed.clamp(0.5, 2.0);

        // Пути отдаём относительными (рабочая папка — piper/): Piper на Windows
        // спотыкается о кириллицу в путях, например в имени пользователя.
        // Звук получаем сырым потоком из stdout, без временных файлов.
        let mut child = Command::new(crate::piper::exe())
            .current_dir(crate::piper::dir())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: без мигающей консоли
            .arg("--model")
            .arg(format!("{stem}.onnx"))
            .args(["--espeak_data", "espeak-ng-data"])
            .args(["--length_scale", &length_scale.to_string()])
            .arg("--output_raw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("не удалось запустить piper.exe")?;
        let mut text = u.text.trim().to_string();
        text.push('\n');
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("нет stdin у piper"))?
            .write_all(text.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() || output.stdout.len() < 2 {
            let err = String::from_utf8_lossy(&output.stderr);
            let tail: String = err.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(anyhow!("piper ({}), модель {stem}: {}", output.status, tail.trim()));
        }

        let samples: Vec<i16> = output
            .stdout
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        if out.is_none() {
            *out = Some(OutputStream::try_default().context("нет устройства вывода звука")?);
        }
        let handle = &out.as_ref().unwrap().1;
        let s = Sink::try_new(handle)?;
        s.set_speed(pitch);
        s.append(SamplesBuffer::new(1, crate::piper::sample_rate(&stem), samples));
        *sink = Some(s);
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Utterance;
    use crate::state::Shared;
    use std::sync::mpsc::Receiver;
    use std::sync::{Arc, Mutex};

    pub fn run(_shared: Arc<Mutex<Shared>>, rx: Receiver<Utterance>) {
        while rx.recv().is_ok() {}
    }
}
