use tokio::process::Command;
use std::process::Stdio;
use crate::error::{Result, EngineError};

pub async fn run_async_muxer(video_path: &str, audio_path: &str, output_path: &str) -> Result<()> {
    let mut child = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i").arg(video_path)
        .arg("-i").arg(audio_path)
        .arg("-c:v").arg("copy")
        .arg("-c:a").arg("aac")
        .arg("-map").arg("0:v:0")
        .arg("-map").arg("1:a:0")
        .arg(output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| EngineError::InternalPanic { reason: format!("Failed to spawn ffmpeg child: {}", e) })?;

    let status = child.wait().await?;

    if !status.success() {
        return Err(EngineError::InternalPanic { reason: "Muxing pipeline compilation failed".to_string() });
    }

    Ok(())
}
