use crate::data::router::StreamRouter;
use crate::download::manager::DownloadManager;
use crate::extractor::traits::{AudioQuality, AudioSource};
use crate::media::converter::AudioFormat;
use crate::media::pipeline::process_downloaded_file;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use url::Url;

/// Downloads a track, processes it (format conversion + tagging), and writes it to `temp_dir`.
/// Returns the absolute path of the completed file. The file is left on disk for the caller
/// (Kotlin) to move via MediaStore.
///
/// # Panics
/// Does not panic; all errors are returned as `Err`.
pub async fn download_to_temp_dir(
    router: Arc<StreamRouter>,
    download_manager: Arc<DownloadManager>,
    url: String,
    quality: AudioQuality,
    format: Option<AudioFormat>,
    temp_dir: String,
) -> Result<PathBuf> {
    // Determine the platform source from the URL
    let source = detect_source_from_url(&url)?;

    // Extract the track ID from the URL
    let track_id = extract_track_id(&url, &source)?;

    // Resolve the stream info (stream URL + metadata)
    let stream_info = router.get_stream(&source, &track_id, quality).await?;

    // Build the temporary output path
    let ext = format
        .as_ref()
        .map(|f| f.to_string())
        .unwrap_or_else(|| {
            // Derive extension from format string (e.g. "audio/webm" → "webm", "audio/mpeg" → "mp3")
            stream_info
                .format
                .split('/')
                .nth(1)
                .and_then(|s| s.split(';').next())
                .map(str::to_string)
                .unwrap_or_else(|| "mp3".to_string())
        });

    let file_name = sanitize_filename(&format!(
        "{} - {}.{}",
        stream_info.metadata.artist, stream_info.metadata.title, ext
    ));
    let temp_path = Path::new(&temp_dir).join(&file_name);

    // Queue the download and wait for completion via a oneshot channel.
    // The DownloadManager worker sends a signal (via watch) when the download is done;
    // we poll the watch channel until our file path appears as completed.
    let stream_url = Url::parse(&stream_info.stream_url)
        .map_err(|e| anyhow!("Invalid stream URL '{}': {}", stream_info.stream_url, e))?;

    let mut progress_rx = download_manager.subscribe_progress();
    download_manager
        .queue_download(stream_url, temp_path.clone())
        .await?;

    // Wait until the download manager signals 100% for this URL
    let target_url = stream_info.stream_url.clone();
    let (done_tx, done_rx) = oneshot::channel::<Result<()>>();
    let mut progress_rx_clone = progress_rx.clone();

    tokio::spawn(async move {
        loop {
            if progress_rx_clone.changed().await.is_err() {
                let _ = done_tx.send(Err(anyhow!("Progress channel closed unexpectedly")));
                return;
            }
            let (url, downloaded, total) = progress_rx_clone.borrow().clone();
            if url == target_url && total > 0 && downloaded >= total {
                let _ = done_tx.send(Ok(()));
                return;
            }
        }
    });

    done_rx
        .await
        .map_err(|_| anyhow!("Download completion signal lost"))??;

    // Post-process: convert format + embed tags
    let (progress_tx, _) = mpsc::channel(16);
    let cover_art_url = stream_info.metadata.album_art_url.clone();
    let metadata_for_tag = stream_info.metadata.clone();

    process_downloaded_file(
        &temp_path,
        &metadata_for_tag,
        &cover_art_url,
        format,
        progress_tx,
    )
    .await?;

    Ok(temp_path)
}

/// Detects the `AudioSource` from a URL string.
fn detect_source_from_url(url: &str) -> Result<AudioSource> {
    if url.contains("youtube.com") || url.contains("youtu.be") || url.contains("music.youtube.com") {
        Ok(AudioSource::YouTubeMusic)
    } else if url.contains("soundcloud.com") {
        Ok(AudioSource::SoundCloud)
    } else if url.contains("spotify.com") {
        Ok(AudioSource::Spotify)
    } else {
        Err(anyhow!("Cannot determine platform from URL: {}", url))
    }
}

/// Extracts the platform-specific track ID from a URL.
fn extract_track_id(url: &str, source: &AudioSource) -> Result<String> {
    match source {
        AudioSource::YouTubeMusic => {
            // Extract ?v=VIDEO_ID or youtu.be/VIDEO_ID
            if let Some(v_pos) = url.find("v=") {
                let after = &url[v_pos + 2..];
                let id = after.split('&').next().unwrap_or(after);
                if id.len() == 11 {
                    return Ok(id.to_string());
                }
            }
            if url.contains("youtu.be/") {
                let after = url.split("youtu.be/").nth(1).unwrap_or("");
                let id = after.split('?').next().unwrap_or(after);
                if id.len() == 11 {
                    return Ok(id.to_string());
                }
            }
            Err(anyhow!("Could not extract YouTube video ID from '{}'", url))
        }
        AudioSource::SoundCloud => {
            // For SoundCloud, the track ID may already be numeric or the URL path is used
            // as the query in get_stream_url; return the URL itself as the ID
            Ok(url.to_string())
        }
        AudioSource::Spotify => {
            Err(anyhow!("Spotify streaming is not supported"))
        }
    }
}

/// Sanitises a file name by removing characters not allowed in most file systems.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}
