use std::path::Path;
use std::process::Stdio;
use anyhow::{anyhow, Result};
use tokio::process::Command;
use which::which;
use tokio::sync::mpsc;
use tokio::io::{BufReader, AsyncBufReadExt};

#[derive(Debug, Clone, Copy)]
pub enum AudioFormat {
    Flac,
    Wav,
    Mp3,
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AudioFormat::Flac => "flac",
            AudioFormat::Wav => "wav",
            AudioFormat::Mp3 => "mp3",
        };
        write!(f, "{}", s)
    }
}

pub async fn convert_format(
    input_path: &Path,
    output_path: &Path,
    target_format: AudioFormat,
    progress_sender: mpsc::Sender<String>,
) -> Result<()> {
    if which("ffmpeg").is_err() {
        return Err(anyhow!("ffmpeg not found in PATH"));
    }

    let codec = match target_format {
        AudioFormat::Flac => "flac",
        AudioFormat::Wav => "pcm_s16le",
        AudioFormat::Mp3 => "libmp3lame",
    };

    let mut command = Command::new("ffmpeg");
    command
        .arg("-i")
        .arg(input_path)
        .arg("-c:a")
        .arg(codec)
        .arg("-b:a")
        .arg("320k")
        .arg(output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("Failed to capture ffmpeg stderr"))?;
    let mut reader = BufReader::new(stderr).lines();

    while let Some(line) = reader.next_line().await? {
        progress_sender.send(line).await?;
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(anyhow!("ffmpeg conversion failed"));
    }

    tokio::fs::remove_file(input_path).await?;

    Ok(())
}
