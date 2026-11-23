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

    let format_clean = target_format.to_lowercase();
    let (codec_args, container_format): (Vec<&str>, &str) = match format_clean.as_str() {
        "mp3" => (vec!["-codec:a", "libmp3lame", "-qscale:a", "2"], "mp3"),
        "flac" => (vec!["-codec:a", "flac"], "flac"),
        "opus" => (vec!["-codec:a", "libopus", "-b:a", "192k"], "opus"),
        "wav" => (vec!["-codec:a", "pcm_s16le"], "wav"),
        "m4a" => (
            vec![
                "-codec:a",
                "aac",
                "-b:a",
                "256k",
                "-movflags",
                "frag_keyframe+empty_moov",
            ],
            "mp4",
        ),
        "aac" => (vec!["-codec:a", "aac", "-b:a", "256k"], "adts"),
        "vorbis" | "ogg" => (vec!["-codec:a", "libvorbis", "-qscale:a", "4"], "ogg"),
        "best" | "copy" => (vec!["-codec:a", "copy"], "matroska"),
        other => {
            return Err(EngineError::InvalidConfig(format!(
                "Unsupported target audio format: {}",
                other
            )));
        }
    };

    for arg in &codec_args {
        cmd.arg(arg);
    }

    cmd.arg("-f").arg(container_format);
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

/// Determines container-safe FFmpeg video and audio codec arguments to avoid illegal codec container errors.
pub fn determine_mux_codecs(
    video_ext: &str,
    audio_ext: &str,
    out_ext: &str,
) -> (Vec<&'static str>, Vec<&'static str>) {
    let v_clean = video_ext.trim_start_matches('.').to_lowercase();
    let a_clean = audio_ext.trim_start_matches('.').to_lowercase();
    let out_clean = out_ext.trim_start_matches('.').to_lowercase();

    match out_clean.as_str() {
        "webm" => {
            let v_args = if matches!(v_clean.as_str(), "webm" | "vp9" | "vp8" | "av1") {
                vec!["-c:v", "copy"]
            } else {
                vec!["-c:v", "libvpx-vp9"]
            };
            let a_args = if matches!(a_clean.as_str(), "webm" | "opus") {
                vec!["-c:a", "copy"]
            } else {
                vec!["-c:a", "libopus"]
            };
            (v_args, a_args)
        }
        "mkv" => (vec!["-c:v", "copy"], vec!["-c:a", "copy"]),
        _ => {
            let v_args = vec!["-c:v", "copy"];
            let a_args = if matches!(a_clean.as_str(), "m4a" | "aac") {
                vec!["-c:a", "copy"]
            } else {
                vec!["-c:a", "aac"]
            };
            (v_args, a_args)
        }
    }
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
        .arg(audio_path);

    let out_ext = output_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let video_ext = video_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let audio_ext = audio_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let (v_codecs, a_codecs) = determine_mux_codecs(video_ext, audio_ext, out_ext);
    for arg in v_codecs {
        cmd.arg(arg);
    }
    for arg in a_codecs {
        cmd.arg(arg);
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
    mux_streams_piped_with_buffer(
        video_rx,
        audio_rx,
        output_path,
        video_ext,
        audio_ext,
        cancel_token,
        crate::downloader::flusher::default_streaming_buffer_size(),
    )
    .await
}

/// On-the-fly concurrent dual-stream muxing using kernel FIFOs with configurable buffer size.
pub async fn mux_streams_piped_with_buffer(
    video_rx: tokio::sync::mpsc::Receiver<Result<SequencedChunk, EngineError>>,
    audio_rx: tokio::sync::mpsc::Receiver<Result<SequencedChunk, EngineError>>,
    output_path: &Path,
    video_ext: &str,
    audio_ext: &str,
    cancel_token: CancellationToken,
    buffer_size_bytes: usize,
) -> Result<u64, EngineError> {
    let ffmpeg_bin = resolve_ffmpeg_path()?;
    let is_stdout = output_path == Path::new("-");

    if cancel_token.is_cancelled() {
        return Err(EngineError::Cancelled);
    }

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

        let _ = tokio::fs::remove_file(&v_fifo).await;
        let _ = tokio::fs::remove_file(&a_fifo).await;

        let v_c = CString::new(v_fifo.to_string_lossy().as_bytes())
            .map_err(|e| EngineError::IoError(e.to_string()))?;
        let a_c = CString::new(a_fifo.to_string_lossy().as_bytes())
            .map_err(|e| EngineError::IoError(e.to_string()))?;

        unsafe {
            let res_v = libc::mkfifo(v_c.as_ptr(), 0o600);
            if res_v != 0 {
                return Err(EngineError::IoError(format!(
                    "Failed to create video FIFO at {}: {}",
                    v_fifo.display(),
                    std::io::Error::last_os_error()
                )));
            }
            let res_a = libc::mkfifo(a_c.as_ptr(), 0o600);
            if res_a != 0 {
                let _ = tokio::fs::remove_file(&v_fifo).await;
                return Err(EngineError::IoError(format!(
                    "Failed to create audio FIFO at {}: {}",
                    a_fifo.display(),
                    std::io::Error::last_os_error()
                )));
            }
        }

        let mut cmd = Command::new(&ffmpeg_bin);
        cmd.kill_on_drop(true);
        cmd.arg("-y").arg("-i").arg(&v_fifo).arg("-i").arg(&a_fifo);

        let out_ext = if is_stdout {
            if video_ext == "webm" {
                "webm"
            } else if video_ext == "mkv" {
                "mkv"
            } else {
                "mp4"
            }
        } else {
            output_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or(video_ext)
        };

        let (v_codecs, a_codecs) = determine_mux_codecs(video_ext, audio_ext, out_ext);
        for arg in v_codecs {
            cmd.arg(arg);
        }
        for arg in a_codecs {
            cmd.arg(arg);
        }

        if is_stdout {
            if out_ext == "webm" {
                cmd.arg("-f").arg("webm");
            } else if out_ext == "mkv" {
                cmd.arg("-f").arg("matroska");
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
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::io::AsRawFd;
                unsafe {
                    libc::fcntl(file.as_raw_fd(), libc::F_SETPIPE_SZ, 1048576);
                }
            }
            let flusher = SequentialFlusher::new().with_max_buffer_bytes(buffer_size_bytes);
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
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::io::AsRawFd;
                unsafe {
                    libc::fcntl(file.as_raw_fd(), libc::F_SETPIPE_SZ, 1048576);
                }
            }
            let flusher = SequentialFlusher::new().with_max_buffer_bytes(buffer_size_bytes);
            flusher.flush_live_to_writer(file, audio_rx, a_token).await
        });

        let stdout_handle = if is_stdout && let Some(mut stdout_pipe) = child.stdout.take() {
            let s_token = cancel_token.clone();
            tokio::spawn(async move {
                let mut out = tokio::io::stdout();
                let mut buf = [0u8; 32768];
                let mut total = 0u64;
                loop {
                    tokio::select! {
                        _ = s_token.cancelled() => {
                            break;
                        }
                        read_res = stdout_pipe.read(&mut buf) => {
                            let n = read_res.map_err(|e| EngineError::IoError(e.to_string()))?;
                            if n == 0 {
                                break;
                            }
                            out.write_all(&buf[..n])
                                .await
                                .map_err(|e| EngineError::IoError(e.to_string()))?;
                            total += n as u64;
                        }
                    }
                }
                let _ = out.flush().await;
                Ok::<u64, EngineError>(total)
            })
        } else {
            tokio::spawn(async move { Ok::<u64, EngineError>(0) })
        };

        let wait_fut = async {
            let (v_res, a_res, stdout_res) = tokio::join!(v_handle, a_handle, stdout_handle);
            let status = child.wait().await;
            (v_res, a_res, stdout_res, status)
        };

        let (v_res, a_res, stdout_res, status_res) = tokio::select! {
            _ = cancel_token.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = tokio::fs::remove_file(&v_fifo).await;
                let _ = tokio::fs::remove_file(&a_fifo).await;
                return Err(EngineError::Cancelled);
            }
            res = wait_fut => res,
        };

        let _ = tokio::fs::remove_file(&v_fifo).await;
        let _ = tokio::fs::remove_file(&a_fifo).await;

        let v_bytes = v_res.map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;
        let a_bytes = a_res.map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;
        let stdout_bytes =
            stdout_res.map_err(|e| EngineError::ConcurrencyError(e.to_string()))??;

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

    let format_clean = target_format.to_lowercase();
    match format_clean.as_str() {
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
        "m4a" => {
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("256k");
        }
        "aac" => {
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("256k");
        }
        "flac" => {
            cmd.arg("-c:a").arg("flac");
        }
        "opus" => {
            cmd.arg("-c:a").arg("libopus").arg("-b:a").arg("192k");
        }
        "vorbis" | "ogg" => {
            cmd.arg("-c:a").arg("libvorbis");
        }
        "wav" => {
            cmd.arg("-c:a").arg("pcm_s16le");
        }
        "best" | "copy" => {
            cmd.arg("-c:a").arg("copy");
        }
        _ => {
            cmd.arg("-c:a").arg("copy");
        }
    }

    let is_stdout = output_path == Path::new("-");
    if is_stdout {
        let container = match format_clean.as_str() {
            "mp3" => "mp3",
            "flac" => "flac",
            "opus" => "opus",
            "wav" => "wav",
            "m4a" => {
                cmd.arg("-movflags").arg("frag_keyframe+empty_moov");
                "mp4"
            }
            "aac" => "adts",
            "vorbis" | "ogg" => "ogg",
            "best" | "copy" => "matroska",
            other => other,
        };
        cmd.arg("-f").arg(container);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_mux_codecs_webm() {
        // VP9 + Opus -> copy/copy
        let (v, a) = determine_mux_codecs("webm", "opus", "webm");
        assert_eq!(v, vec!["-c:v", "copy"]);
        assert_eq!(a, vec!["-c:a", "copy"]);

        // MP4 (H.264) + AAC -> transcode video to VP9 and audio to libopus
        let (v, a) = determine_mux_codecs("mp4", "aac", "webm");
        assert_eq!(v, vec!["-c:v", "libvpx-vp9"]);
        assert_eq!(a, vec!["-c:a", "libopus"]);

        // MP4 (H.264) + Opus -> transcode video to VP9 and copy Opus audio
        let (v, a) = determine_mux_codecs("mp4", "opus", "webm");
        assert_eq!(v, vec!["-c:v", "libvpx-vp9"]);
        assert_eq!(a, vec!["-c:a", "copy"]);
    }

    #[test]
    fn test_determine_mux_codecs_mp4() {
        // MP4 + M4A (AAC) -> copy/copy
        let (v, a) = determine_mux_codecs("mp4", "m4a", "mp4");
        assert_eq!(v, vec!["-c:v", "copy"]);
        assert_eq!(a, vec!["-c:a", "copy"]);

        // WebM + Opus -> copy video, transcode audio to aac
        let (v, a) = determine_mux_codecs("webm", "opus", "mp4");
        assert_eq!(v, vec!["-c:v", "copy"]);
        assert_eq!(a, vec!["-c:a", "aac"]);
    }

    #[test]
    fn test_determine_mux_codecs_mkv() {
        // Any codec combination into MKV -> lossless copy/copy
        let (v, a) = determine_mux_codecs("webm", "opus", "mkv");
        assert_eq!(v, vec!["-c:v", "copy"]);
        assert_eq!(a, vec!["-c:a", "copy"]);

        let (v, a) = determine_mux_codecs("mp4", "aac", "mkv");
        assert_eq!(v, vec!["-c:v", "copy"]);
        assert_eq!(a, vec!["-c:a", "copy"]);
    }
}
