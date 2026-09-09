use lofty::file::AudioFile;
use lofty::probe::Probe;
use lofty::tag::{Accessor, Tag, TagType};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::config::MediaMetadata;
use crate::error::EngineError;

pub fn resolve_ffmpeg_path() -> Result<PathBuf, EngineError> {
    which::which("ffmpeg")
        .map_err(|_| EngineError::FfmpegNotFound)
}

pub async fn transcode_stream(
    mut input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    output_path: &Path,
    target_format: &str,
) -> Result<(), EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.arg("-i").arg("pipe:0")
       .arg("-y");

    match target_format.to_lowercase().as_str() {
        "mp3" => {
            cmd.arg("-codec:a").arg("libmp3lame").arg("-qscale:a").arg("2");
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

    let mut child = cmd.spawn().map_err(|e| EngineError::FfmpegError(e.to_string()))?;
    let mut stdin = child.stdin.take().ok_or_else(|| EngineError::FfmpegError("Failed to open stdin".to_string()))?;
    let mut stdout = child.stdout.take().ok_or_else(|| EngineError::FfmpegError("Failed to open stdout".to_string()))?;

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

    let (stdin_res, stdout_res, status_res) = tokio::join!(write_stdin_task, read_stdout_task, child.wait());
    
    stdin_res.map_err(|e| EngineError::FfmpegError(e.to_string()))?;
    stdout_res.map_err(|e| EngineError::FfmpegError(e.to_string()))?.map_err(|e| EngineError::IoError(e.to_string()))?;
    
    let status = status_res.map_err(|e| EngineError::FfmpegError(e.to_string()))?;
    if !status.success() {
        return Err(EngineError::FfmpegError("FFmpeg process returned non-zero exit code".to_string()));
    }

    Ok(())
}

pub async fn download_cover_art(client: &Client, url: &str) -> Option<Vec<u8>> {
    client.get(url).send().await.ok()?.bytes().await.ok().map(|b| b.to_vec())
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

    let tag_type = tagged_file.primary_tag_type().unwrap_or(TagType::Id3v2);
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

        let picture = lofty::picture::Picture::new_unchecked(
            lofty::picture::PictureType::CoverFront,
            Some(mime),
            None,
            art_bytes,
        );
        tag.push_picture(picture);
    }

    tagged_file
        .save_to_path(file_path, lofty::config::WriteOptions::default())
        .map_err(|e| EngineError::TaggingError(e.to_string()))?;

    Ok(())
}
