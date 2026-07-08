use crate::data::router::StreamRouter;
use crate::data::playlist::{json_handler};
use crate::domain::playlist::Playlist as DomainPlaylist;
use crate::ffi::types::{AudioSource, AudioQuality, Playlist, TrackMetadata};
use crate::media::converter::AudioFormat as DomainAudioFormat;
use crate::ffi::file_handoff::download_to_temp_dir;
use crate::download::manager::DownloadManager;
use anyhow::Result;
use std::panic::catch_unwind;
use std::sync::Arc;
use uniffi::export;

#[derive(uniffi::Enum, Clone, Copy)]
pub enum AudioFormat {
    Flac,
    Wav,
    Mp3,
}

impl From<AudioFormat> for DomainAudioFormat {
    fn from(format: AudioFormat) -> Self {
        match format {
            AudioFormat::Flac => Self::Flac,
            AudioFormat::Wav => Self::Wav,
            AudioFormat::Mp3 => Self::Mp3,
        }
    }
}

pub struct MmDlpApi {
    router: Arc<StreamRouter>,
    downloader: Arc<DownloadManager>,
}

impl MmDlpApi {
    pub fn new() -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let router = rt.block_on(StreamRouter::new())?;
        let downloader = DownloadManager::new()?;
        Ok(Self {
            router: Arc::new(router),
            downloader: Arc::new(downloader),
        })
    }
}

#[export]
impl MmDlpApi {
    pub fn search(&self, query: String, source: AudioSource) -> Result<Vec<TrackMetadata>> {
        let result = catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let source: crate::extractor::traits::AudioSource = source.into();
            rt.block_on(async {
                let results = match source {
                    crate::extractor::traits::AudioSource::YouTubeMusic => self.router.youtube_extractor.search(&query).await,
                    crate::extractor::traits::AudioSource::SoundCloud => self.router.soundcloud_extractor.search(&query).await,
                    _ => unimplemented!(),
                }?;
                Ok(results.into_iter().map(|t| t.into()).collect())
            })
        });
        result.unwrap_or_else(|_| Err(anyhow::anyhow!("Search panicked")))
    }

    pub fn download_track(&self, url: String, quality: AudioQuality, format: Option<AudioFormat>, temp_dir: String) -> Result<String> {
        let result = catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                let path = download_to_temp_dir(
                    self.router.clone(),
                    self.downloader.clone(),
                    url,
                    quality.into(),
                    format.map(|f| f.into()),
                    temp_dir,
                )
                .await?;
                Ok(path.to_str().unwrap().to_string())
            })
        });
        result.unwrap_or_else(|_| Err(anyhow::anyhow!("Download panicked")))
    }

    pub fn export_playlist_json(&self, playlist: Playlist) -> Result<String> {
        let domain_playlist: DomainPlaylist = playlist.into();
        json_handler::export_to_json(&domain_playlist).map_err(|e| e.into())
    }

    pub fn import_playlist_json(&self, json: String) -> Result<Playlist> {
        let domain_playlist = json_handler::import_from_json(&json)?;
        Ok(domain_playlist.into())
    }
}

impl From<crate::extractor::traits::TrackMetadata> for TrackMetadata {
    fn from(metadata: crate::extractor::traits::TrackMetadata) -> Self {
        Self {
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            album_art_url: metadata.album_art_url,
            track_id: metadata.track_id,
            source: metadata.source.into(),
        }
    }
}
