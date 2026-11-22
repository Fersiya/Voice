use crate::config::Piper;
use anyhow::{anyhow, Context, Result};
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::{BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub struct Utterance {
    pub text: String,
    pub speaker: u32,
}

/// Рабочий поток озвучки. Новая реплика прерывает текущую,
/// старые необработанные реплики пропускаются (озвучиваем только актуальное).
pub fn run(cfg: Piper, rx: Receiver<Utterance>) -> Result<()> {
    let (_stream, handle) = OutputStream::try_default().context("нет устройства вывода звука")?;
    let wav = std::env::temp_dir().join("voice_tts.wav");
    let mut sink: Option<Sink> = None;

    loop {
        let mut next = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(u) => u,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        while let Ok(newer) = rx.try_recv() {
            next = newer;
        }

        if let Some(s) = sink.take() {
            s.stop();
        }

        if let Err(e) = synth(&cfg, &next, &wav) {
            eprintln!("TTS: {e:#}");
            continue;
        }
        let file = BufReader::new(File::open(&wav)?);
        let s = Sink::try_new(&handle)?;
        s.append(Decoder::new(file)?);
        sink = Some(s);
    }
}

fn synth(cfg: &Piper, u: &Utterance, out: &std::path::Path) -> Result<()> {
    let mut child = Command::new(&cfg.exe)
        .args(["--model", &cfg.model])
        .args(["--length_scale", &cfg.length_scale.to_string()])
        .args(["--speaker", &u.speaker.to_string()])
        .arg("--output_file")
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("не удалось запустить {}", cfg.exe))?;

    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("нет stdin у piper"))?
        .write_all(u.text.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("piper завершился с кодом {status}"));
    }
    Ok(())
}
