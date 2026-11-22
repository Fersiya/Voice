use crate::state::Shared;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

pub struct Utterance {
    pub text: String,
    /// Windows: имя голоса. Piper: номер спикера. Пусто — по умолчанию.
    pub voice: String,
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
    use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
    use std::fs::File;
    use std::io::{BufReader, Write};
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
        let wav = std::env::temp_dir().join("voice_tts.wav");

        while let Ok(mut u) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                u = newer;
            }
            let speech = shared.lock().unwrap().cfg.speech.clone();

            let res = match speech.engine {
                Engine::Windows => speak_windows(&mut sys, &speech, &u),
                Engine::Piper => speak_piper(&mut out, &mut sink, &speech, &u, &wav),
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

        t.speak(&u.text, true).map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }

    fn speak_piper(
        out: &mut Option<(OutputStream, OutputStreamHandle)>,
        sink: &mut Option<Sink>,
        speech: &Speech,
        u: &Utterance,
        wav: &std::path::Path,
    ) -> Result<()> {
        if let Some(s) = sink.take() {
            s.stop();
        }

        let speaker: u32 = if u.voice.is_empty() { &speech.default_voice } else { &u.voice }
            .trim()
            .parse()
            .unwrap_or(0);
        let length_scale = 1.0 / speech.speed.clamp(0.5, 2.0);

        let mut child = Command::new(&speech.piper_exe)
            .args(["--model", &speech.piper_model])
            .args(["--length_scale", &length_scale.to_string()])
            .args(["--speaker", &speaker.to_string()])
            .arg("--output_file")
            .arg(wav)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("не удалось запустить {}", speech.piper_exe))?;
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("нет stdin у piper"))?
            .write_all(u.text.as_bytes())?;
        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow!("piper завершился с ошибкой ({status})"));
        }

        if out.is_none() {
            *out = Some(OutputStream::try_default().context("нет устройства вывода звука")?);
        }
        let handle = &out.as_ref().unwrap().1;
        let s = Sink::try_new(handle)?;
        s.append(Decoder::new(BufReader::new(File::open(wav)?))?);
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
