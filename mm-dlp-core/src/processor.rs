use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, Tag};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::config::MediaMetadata;
use crate::downloader::flusher::SequentialFlusher;
use crate::downloader::parallel::SequencedChunk;
use crate::error::EngineError;

pub fn resolve_ffmpeg_path() -> Result<PathBuf, EngineError> {
    which::which("ffmpeg").map_err(|_| EngineError::FfmpegNotFound)
}

pub async fn transcode_stream(
    input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    output_path: &Path,
    target_format: &str,
    loudness_norm: bool,
) -> Result<(), EngineError> {
    let out_file = tokio::fs::File::create(output_path).await?;
    transcode_stream_to_writer(input_rx, out_file, target_format, loudness_norm).await
}

/// Transcode an audio stream on the fly and write directly to an asynchronous writer (stdout or file).
pub async fn transcode_stream_to_writer<W: AsyncWrite + Unpin + Send + 'static>(
    mut input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    mut writer: W,
    target_format: &str,
    loudness_norm: bool,
) -> Result<(), EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.arg("-i").arg("pipe:0").arg("-y");

    if loudness_norm {
        cmd.arg("-af").arg("loudnorm=I=-16:TP=-1.5:LRA=11");
    }

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

    let read_stdout_task = tokio::spawn(async move {
        let mut buf = [0u8; 16384];
        loop {
            let n = stdout.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await?;
        }
        writer.flush().await?;
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
            tagged_file
                .primary_tag_mut()
                .ok_or_else(|| EngineError::TaggingError("Failed to access primary tag".into()))?
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
        .arg("copy");

    let out_ext = output_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let audio_ext = audio_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    match out_ext.as_str() {
        "webm" => {
            if audio_ext == "webm" || audio_ext == "opus" {
                cmd.arg("-c:a").arg("copy");
            } else {
                cmd.arg("-c:a").arg("libopus");
            }
        }
        "mp4" | "m4v" | "m4a" => {
            if audio_ext == "m4a" || audio_ext == "aac" {
                cmd.arg("-c:a").arg("copy");
            } else {
                cmd.arg("-c:a").arg("aac");
            }
        }
        "mkv" => {
            cmd.arg("-c:a").arg("copy");
        }
        _ => {
            // General fallback: try copy first, ffmpeg will transcode if impossible
            cmd.arg("-c:a").arg("copy");
        }
    }

    cmd.arg(output_path);

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

static FIFO_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// On-the-fly concurrent dual-stream muxing using kernel FIFOs.
///
/// Feeds video and audio chunks directly into FFmpeg input pipes simultaneously without writing
/// intermediate video/audio files to disk. Streams the resulting fragmented MP4 or WebM directly to stdout or file.
pub async fn mux_streams_piped(
    video_rx: tokio::sync::mpsc::Receiver<Result<SequencedChunk, EngineError>>,
    audio_rx: tokio::sync::mpsc::Receiver<Result<SequencedChunk, EngineError>>,
    output_path: &Path,
    video_ext: &str,
    audio_ext: &str,
    cancel_token: CancellationToken,
) -> Result<u64, EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;
    let is_stdout = output_path == Path::new("-");

    #[cfg(unix)]
    {
        use std::ffi::CString;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let count = FIFO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let rand_id = format!("{}_{}_{}", std::process::id(), nanos, count);
        let v_fifo = std::env::temp_dir().join(format!("mm_v_{}.fifo", rand_id));
        let a_fifo = std::env::temp_dir().join(format!("mm_a_{}.fifo", rand_id));

        let v_c = CString::new(v_fifo.to_string_lossy().as_bytes())
            .map_err(|e| EngineError::IoError(e.to_string()))?;
        let a_c = CString::new(a_fifo.to_string_lossy().as_bytes())
            .map_err(|e| EngineError::IoError(e.to_string()))?;

        unsafe {
            libc::mkfifo(v_c.as_ptr(), 0o600);
            libc::mkfifo(a_c.as_ptr(), 0o600);
        }

        let mut cmd = Command::new(&ffmpeg_bin);
        cmd.kill_on_drop(true);
        cmd.arg("-y")
            .arg("-i")
            .arg(&v_fifo)
            .arg("-i")
            .arg(&a_fifo)
            .arg("-c:v")
            .arg("copy");

        let out_ext = if is_stdout {
            if video_ext == "webm" { "webm" } else { "mp4" }
        } else {
            output_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or(video_ext)
        };

        match out_ext {
            "webm" => {
                if audio_ext == "webm" || audio_ext == "opus" {
                    cmd.arg("-c:a").arg("copy");
                } else {
                    cmd.arg("-c:a").arg("libopus");
                }
            }
            _ => {
                if audio_ext == "m4a" || audio_ext == "aac" {
                    cmd.arg("-c:a").arg("copy");
                } else {
                    cmd.arg("-c:a").arg("aac");
                }
            }
        }

        if is_stdout {
            if out_ext == "webm" {
                cmd.arg("-f").arg("webm");
            } else {
                cmd.arg("-f")
                    .arg("mp4")
                    .arg("-movflags")
                    .arg("frag_keyframe+empty_moov+default_base_moof");
            }
            cmd.arg("pipe:1");
            cmd.stdout(Stdio::piped());
        } else {
            cmd.arg(output_path);
            cmd.stdout(Stdio::null());
        }
        cmd.stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| EngineError::FfmpegError(e.to_string()))?;

        let v_path = v_fifo.clone();
        let v_token = cancel_token.clone();
        let v_handle = tokio::spawn(async move {
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&v_path)
                .await
                .map_err(|e| EngineError::IoError(e.to_string()))?;
            let flusher = SequentialFlusher::new();
            flusher.flush_live_to_writer(file, video_rx, v_token).await
        });

        let a_path = a_fifo.clone();
        let a_token = cancel_token.clone();
        let a_handle = tokio::spawn(async move {
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&a_path)
                .await
                .map_err(|e| EngineError::IoError(e.to_string()))?;
            let flusher = SequentialFlusher::new();
            flusher.flush_live_to_writer(file, audio_rx, a_token).await
        });

        let mut stdout_bytes = 0u64;
        if is_stdout && let Some(mut stdout_pipe) = child.stdout.take() {
            let mut out = tokio::io::stdout();
            let mut buf = [0u8; 32768];
            loop {
                let n = stdout_pipe
                    .read(&mut buf)
                    .await
                    .map_err(|e| EngineError::IoError(e.to_string()))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])
                    .await
                    .map_err(|e| EngineError::IoError(e.to_string()))?;
                stdout_bytes += n as u64;
            }
            let _ = out.flush().await;
        }

        let (v_res, a_res, status_res) = tokio::join!(v_handle, a_handle, child.wait());

        let _ = tokio::fs::remove_file(&v_fifo).await;
        let _ = tokio::fs::remove_file(&a_fifo).await;

        let v_bytes = v_res.map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;
        let a_bytes = a_res.map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;

        let status = status_res.map_err(|e| EngineError::FfmpegError(e.to_string()))?;
        if !status.success() && !cancel_token.is_cancelled() {
            return Err(EngineError::FfmpegError(
                "FFmpeg muxing process failed".into(),
            ));
        }

        if is_stdout {
            Ok(stdout_bytes)
        } else {
            Ok(v_bytes + a_bytes)
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (video_rx, audio_rx, video_ext, audio_ext, cancel_token);
        Err(EngineError::IoError(
            "On-the-fly piped muxing requires Unix FIFO support".into(),
        ))
    }
}

pub async fn extract_audio_from_file(
    input_path: &Path,
    output_path: &Path,
    target_format: &str,
    target_quality: &str,
    loudness_norm: bool,
) -> Result<(), EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;
    let mut cmd = Command::new(ffmpeg_bin);
    cmd.kill_on_drop(true);
    cmd.arg("-y").arg("-i").arg(input_path).arg("-vn");

    if loudness_norm {
        cmd.arg("-af").arg("loudnorm=I=-16:TP=-1.5:LRA=11");
    }

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

    let is_stdout = output_path == Path::new("-");
    if is_stdout {
        cmd.arg("-f").arg(target_format);
        cmd.arg("pipe:1");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| EngineError::FfmpegError(e.to_string()))?;
        if let Some(mut stdout) = child.stdout.take() {
            let mut out = tokio::io::stdout();
            let mut buf = [0u8; 16384];
            loop {
                let n = stdout
                    .read(&mut buf)
                    .await
                    .map_err(|e| EngineError::IoError(e.to_string()))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])
                    .await
                    .map_err(|e| EngineError::IoError(e.to_string()))?;
            }
            out.flush()
                .await
                .map_err(|e| EngineError::IoError(e.to_string()))?;
        }
        let status = child
            .wait()
            .await
            .map_err(|e| EngineError::FfmpegError(e.to_string()))?;
        if !status.success() {
            return Err(EngineError::FfmpegError(
                "FFmpeg audio extractor failed".into(),
            ));
        }
        return Ok(());
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
