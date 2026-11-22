use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, Tag};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::config::MediaMetadata;
use crate::error::EngineError;

pub fn resolve_ffmpeg_path() -> Result<PathBuf, EngineError> {
    which::which("ffmpeg").map_err(|_| EngineError::FfmpegNotFound)
}

pub async fn transcode_stream(
    mut input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    output_path: &Path,
    target_format: &str,
) -> Result<(), EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.arg("-i").arg("pipe:0").arg("-y");

    match target_format.to_lowercase().as_str() {
        "mp3" => {
            cmd.arg("-codec:a")
                .arg("libmp3lame")
                .arg("-qscale:a")
                .arg("2");
        }
        "flac" => {
            cmd.arg("-codec:a").arg("flac");
        }
        "opus" => {
            cmd.arg("-codec:a").arg("libopus").arg("-b:a").arg("192k");
        }
        "wav" => {
            cmd.arg("-codec:a").arg("pcm_s16le");
        }
        _ => {
            return Err(EngineError::InvalidConfig(format!(
                "Unsupported target audio format: {}",
                target_format
            )));
        }
    }

    cmd.arg("-f").arg(target_format);
    cmd.arg("pipe:1");

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| EngineError::FfmpegError(e.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| EngineError::FfmpegError("Failed to open stdin".to_string()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| EngineError::FfmpegError("Failed to open stdout".to_string()))?;

    let write_stdin_task = tokio::spawn(async move {
        while let Some(bytes) = input_rx.recv().await {
            if stdin.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });

    let out_path = output_path.to_path_buf();
    let read_stdout_task = tokio::spawn(async move {
        let mut file = tokio::fs::File::create(&out_path).await?;
        let mut buf = [0u8; 16384];
        loop {
            let n = stdout.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).await?;
        }
        file.flush().await?;
        Ok::<(), std::io::Error>(())
    });

    let (stdin_res, stdout_res, status_res) =
        tokio::join!(write_stdin_task, read_stdout_task, child.wait());

    stdin_res.map_err(|e| EngineError::FfmpegError(e.to_string()))?;
    stdout_res
        .map_err(|e| EngineError::FfmpegError(e.to_string()))?
        .map_err(|e| EngineError::IoError(e.to_string()))?;

    let status = status_res.map_err(|e| EngineError::FfmpegError(e.to_string()))?;
    if !status.success() {
        return Err(EngineError::FfmpegError(
            "FFmpeg process returned non-zero exit code".to_string(),
        ));
    }

    Ok(())
}

pub async fn download_cover_art(client: &Client, url: &str) -> Option<Vec<u8>> {
    client
        .get(url)
        .send()
        .await
        .ok()?
        .bytes()
        .await
        .ok()
        .map(|b| b.to_vec())
}

pub fn tag_file_in_place(
    file_path: &Path,
    metadata: &MediaMetadata,
    cover_art: Option<Vec<u8>>,
) -> Result<(), EngineError> {
    let mut tagged_file = Probe::open(file_path)
        .map_err(|e| EngineError::TaggingError(e.to_string()))?
        .read()
        .map_err(|e| EngineError::TaggingError(e.to_string()))?;

    let tag_type = tagged_file.primary_tag_type();
    let tag = match tagged_file.tag_mut(tag_type) {
        Some(t) => t,
        None => {
            tagged_file.insert_tag(Tag::new(tag_type));
            tagged_file.primary_tag_mut().unwrap()
        }
    };

    tag.set_title(metadata.title.clone());
    tag.set_artist(metadata.artist.clone());
    if let Some(ref alb) = metadata.album {
        tag.set_album(alb.clone());
    }

    if let Some(art_bytes) = cover_art {
        let mime = if art_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            lofty::picture::MimeType::Jpeg
        } else {
            lofty::picture::MimeType::Png
        };

        let builder = lofty::picture::Picture::unchecked(art_bytes)
            .pic_type(lofty::picture::PictureType::CoverFront)
            .mime_type(mime);
        let picture = builder.build();
        tag.push_picture(picture);
    }

    tagged_file
        .save_to_path(file_path, lofty::config::WriteOptions::default())
        .map_err(|e| EngineError::TaggingError(e.to_string()))?;

    Ok(())
}

pub async fn mux_streams_to_file(
    video_path: &Path,
    audio_path: &Path,
    output_path: &Path,
) -> Result<(), EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;
    let mut cmd = Command::new(ffmpeg_bin);
    cmd.kill_on_drop(true);
    cmd.arg("-y")
        .arg("-i")
        .arg(video_path)
        .arg("-i")
        .arg(audio_path)
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg(output_path);

    let status = cmd
        .status()
        .await
        .map_err(|e| EngineError::FfmpegError(format!("Failed to execute ffmpeg muxer: {}", e)))?;

    if status.success() {
        Ok(())
    } else {
        Err(EngineError::FfmpegError(format!(
            "ffmpeg exited with status: {}",
            status
        )))
    }
}

pub async fn extract_audio_from_file(
    input_path: &Path,
    output_path: &Path,
    target_format: &str,
    target_quality: &str,
) -> Result<(), EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;
    let mut cmd = Command::new(ffmpeg_bin);
    cmd.kill_on_drop(true);
    cmd.arg("-y").arg("-i").arg(input_path).arg("-vn");

    match target_format.to_lowercase().as_str() {
        "mp3" => {
            cmd.arg("-c:a").arg("libmp3lame");
            if target_quality == "best" || target_quality == "0" {
                cmd.arg("-q:a").arg("0");
            } else if target_quality.ends_with('k') || target_quality.ends_with('K') {
                cmd.arg("-b:a").arg(target_quality);
            } else {
                cmd.arg("-q:a").arg("2");
            }
        }
        "m4a" | "aac" => {
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("256k");
        }
        "flac" => {
            cmd.arg("-c:a").arg("flac");
        }
        "opus" => {
            cmd.arg("-c:a").arg("libopus").arg("-b:a").arg("192k");
        }
        "wav" => {
            cmd.arg("-c:a").arg("pcm_s16le");
        }
        _ => {
            cmd.arg("-c:a").arg("copy");
        }
    }

    cmd.arg(output_path);

    let status = cmd.status().await.map_err(|e| {
        EngineError::FfmpegError(format!("Failed to execute ffmpeg audio extractor: {}", e))
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(EngineError::FfmpegError(format!(
            "ffmpeg audio extraction exited with status: {}",
            status
        )))
    }
}
