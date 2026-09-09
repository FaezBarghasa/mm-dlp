use crate::client::EngineError;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct FfmpegProgress {
    pub frame: Option<u64>,
    pub fps: Option<f32>,
    pub q: Option<f32>,
    pub size_kb: Option<u64>,
    pub time: Option<String>,
    pub bitrate: Option<String>,
    pub speed: Option<f32>,
}

pub struct FfmpegMuxer;

impl FfmpegMuxer {
    pub fn new() -> Self {
        Self
    }

    pub async fn mux_video_audio<P1, P2, P3, F>(
        &self,
        video_path: P1,
        audio_path: P2,
        output_path: P3,
        mut progress_callback: F,
    ) -> Result<(), EngineError>
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
        P3: AsRef<Path>,
        F: FnMut(FfmpegProgress) + Send + 'static,
    {
        let mut command = Command::new("ffmpeg");

        // Prevents orphan zombie processes by sending a SIGKILL to the child process on parent future drop/cancellation
        command.kill_on_drop(true);

        command
            .arg("-y")
            .arg("-i")
            .arg(video_path.as_ref().as_os_str())
            .arg("-i")
            .arg(audio_path.as_ref().as_os_str())
            .arg("-c:v")
            .arg("copy")
            .arg("-c:a")
            .arg("aac")
            .arg(output_path.as_ref().as_os_str())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|e| EngineError::OsApiError(format!("Failed to spawn ffmpeg: {}", e)))?;

        let stderr = child.stderr.take().ok_or_else(|| {
            EngineError::OsApiError("Failed to capture stderr from ffmpeg process".into())
        })?;

        let mut reader = BufReader::new(stderr).lines();

        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|e| EngineError::OsApiError(format!("Error reading stderr: {}", e)))?
        {
            if line.starts_with("frame=") || line.contains("size=") {
                let progress = Self::parse_progress_line(&line);
                progress_callback(progress);
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|e| EngineError::OsApiError(format!("Failed to wait on ffmpeg: {}", e)))?;

        if status.success() {
            Ok(())
        } else {
            Err(EngineError::OsApiError(format!("Ffmpeg exited with failure status: {}", status)))
        }
    }

    pub fn parse_progress_line(line: &str) -> FfmpegProgress {
        let mut progress = FfmpegProgress {
            frame: None,
            fps: None,
            q: None,
            size_kb: None,
            time: None,
            bitrate: None,
            speed: None,
        };

        let extract = |key: &str| -> Option<String> {
            if let Some(start) = line.find(key) {
                let after = &line[start + key.len()..];
                let value: String = after
                    .trim_start()
                    .chars()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                if value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            } else {
                None
            }
        };

        if let Some(v) = extract("frame=") { progress.frame = v.parse().ok(); }
        if let Some(v) = extract("fps=") { progress.fps = v.parse().ok(); }
        if let Some(v) = extract("q=") { progress.q = v.parse().ok(); }
        if let Some(v) = extract("size=") { progress.size_kb = v.replace("kB", "").parse().ok(); }
        if let Some(v) = extract("time=") { progress.time = Some(v); }
        if let Some(v) = extract("bitrate=") { progress.bitrate = Some(v); }
        if let Some(v) = extract("speed=") { progress.speed = v.replace("x", "").parse().ok(); }

        progress
    }
}

impl Default for FfmpegMuxer {
    fn default() -> Self {
        Self::new()
    }
}