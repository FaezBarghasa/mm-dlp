use crate::data::router::StreamRouter;
use crate::download::manager::DownloadManager;
use crate::extractor::traits::{AudioQuality, TrackMetadata};
use crate::media::converter::AudioFormat;
use crate::media::pipeline::process_downloaded_file;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use url::Url;

pub async fn download_to_temp_dir(
    router: Arc<StreamRouter>,
    download_manager: Arc<DownloadManager>,
    url: String,
    quality: AudioQuality,
    format: Option<AudioFormat>,
    temp_dir: String,
) -> Result<PathBuf> {
    let stream_info = router.get_stream(&url.into(), &url, quality.into()).await?;
    let file_name = format!("{}.{}", stream_info.metadata.track_id, stream_info.format);
    let temp_path = Path::new(&temp_dir).join(&file_name);

    download_manager.queue_download(Url::parse(&stream_info.stream_url)?, temp_path.clone()).await?;

    // For now, we'll assume the download completes successfully and immediately
    // In a real implementation, we'd wait for a completion signal from the DownloadManager
    let (tx, _) = mpsc::channel(1);
    process_downloaded_file(
        &temp_path,
        &stream_info.metadata.into(),
        &stream_info.metadata.album_art_url,
        format,
        tx,
    )
    .await?;

    Ok(temp_path)
}

impl From<crate::extractor::traits::StreamInfo> for TrackMetadata {
    fn from(info: crate::extractor::traits::StreamInfo) -> Self {
        Self {
            title: info.metadata.title,
            artist: info.metadata.artist,
            album: info.metadata.album,
            album_art_url: info.metadata.album_art_url,
            track_id: info.metadata.track_id,
            source: info.metadata.source.into(),
        }
    }
}

impl From<crate::extractor::traits::AudioSource> for crate::ffi::types::AudioSource {
    fn from(source: crate::extractor::traits::AudioSource) -> Self {
        match source {
            crate::extractor::traits::AudioSource::YouTubeMusic => Self::YouTubeMusic,
            crate::extractor::traits::AudioSource::SoundCloud => Self::SoundCloud,
            crate::extractor::traits::AudioSource::Spotify => Self::Spotify,
        }
    }
}
