use crate::data::router::StreamRouter;
use crate::data::playlist::{json_handler, xml_handler};
use crate::domain::playlist::Playlist as DomainPlaylist;
use crate::extractor::traits::AudioSource as DomainAudioSource;
use crate::ffi::types::{AudioSource, AudioQuality, Playlist, TrackMetadata};
use crate::ffi::config;
use crate::media::converter::AudioFormat as DomainAudioFormat;
use crate::ffi::file_handoff::download_to_temp_dir;
use crate::download::manager::DownloadManager;
use anyhow::Result;
use std::panic::catch_unwind;
use std::sync::Arc;

/// The UniFFI-exported audio format enum.
#[derive(uniffi::Enum, Clone, Copy, Debug)]
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

/// The top-level UniFFI API surface exposed to Kotlin/Swift.
#[derive(uniffi::Object)]
pub struct MmDlpApi {
    router: Arc<StreamRouter>,
    downloader: Arc<DownloadManager>,
}

#[uniffi::export]
impl MmDlpApi {
    /// Constructs the API object, initialising all extractors.
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>> {
        let result = catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build runtime: {}", e))?;

            let router = rt.block_on(StreamRouter::new())?;
            let (downloader, task_rx) = DownloadManager::new()?;

            // Spawn the download worker inside the dedicated runtime
            downloader.spawn_worker(task_rx);

            Ok(Arc::new(Self {
                router: Arc::new(router),
                downloader: Arc::new(downloader),
            }))
        });

        match result {
            Ok(inner) => inner,
            Err(_) => Err(anyhow::anyhow!("MmDlpApi::new() panicked")),
        }
    }

    /// Searches the given platform for tracks matching `query`.
    pub fn search(&self, query: String, source: AudioSource) -> Result<Vec<TrackMetadata>> {
        catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("Runtime build failed: {}", e))?;

            let domain_source: DomainAudioSource = source.into();
            let results = rt.block_on(self.router.search(&query, &domain_source))?;
            Ok(results.into_iter().map(TrackMetadata::from).collect())
        })
        .unwrap_or_else(|_| Err(anyhow::anyhow!("search() panicked")))
    }

    /// Downloads a track, processes it, and returns the absolute path of the output file.
    /// `temp_dir` must be a writable directory provided by the caller (e.g., `context.cacheDir`).
    pub fn download_track(
        &self,
        url: String,
        quality: AudioQuality,
        format: Option<AudioFormat>,
        temp_dir: String,
    ) -> Result<String> {
        catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("Runtime build failed: {}", e))?;

            rt.block_on(async {
                let path = download_to_temp_dir(
                    Arc::clone(&self.router),
                    Arc::clone(&self.downloader),
                    url,
                    quality.into(),
                    format.map(|f| f.into()),
                    temp_dir,
                )
                .await?;

                path.to_str()
                    .ok_or_else(|| anyhow::anyhow!("Output path is not valid UTF-8"))
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|_| Err(anyhow::anyhow!("download_track() panicked")))
    }

    /// Serialises a playlist to a JSON string.
    pub fn export_playlist_json(&self, playlist: Playlist) -> Result<String> {
        catch_unwind(|| {
            let domain: DomainPlaylist = playlist.into();
            json_handler::export_to_json(&domain)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .unwrap_or_else(|_| Err(anyhow::anyhow!("export_playlist_json() panicked")))
    }

    /// Deserialises a playlist from a JSON string.
    pub fn import_playlist_json(&self, json: String) -> Result<Playlist> {
        catch_unwind(|| {
            let domain = json_handler::import_from_json(&json)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(Playlist::from(domain))
        })
        .unwrap_or_else(|_| Err(anyhow::anyhow!("import_playlist_json() panicked")))
    }

    /// Serialises a playlist to an XML string.
    pub fn export_playlist_xml(&self, playlist: Playlist) -> Result<String> {
        catch_unwind(|| {
            let domain: DomainPlaylist = playlist.into();
            xml_handler::export_to_xml(&domain)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .unwrap_or_else(|_| Err(anyhow::anyhow!("export_playlist_xml() panicked")))
    }

    /// Deserialises a playlist from an XML string.
    pub fn import_playlist_xml(&self, xml: String) -> Result<Playlist> {
        catch_unwind(|| {
            let domain = xml_handler::import_from_xml(&xml)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(Playlist::from(domain))
        })
        .unwrap_or_else(|_| Err(anyhow::anyhow!("import_playlist_xml() panicked")))
    }

    /// Enables or disables QUIC/HTTP3 globally. Takes effect on the next connection.
    pub fn set_network_config(&self, enable_quic: bool) {
        config::set_quic_enabled(enable_quic);
    }
}

// ─── Type conversions from domain → FFI types ────────────────────────────────

impl From<crate::extractor::traits::TrackMetadata> for TrackMetadata {
    fn from(m: crate::extractor::traits::TrackMetadata) -> Self {
        Self {
            title: m.title,
            artist: m.artist,
            album: m.album,
            album_art_url: m.album_art_url,
            track_id: m.track_id,
            source: m.source.into(),
        }
    }
}

impl From<DomainPlaylist> for Playlist {
    fn from(p: DomainPlaylist) -> Self {
        Self {
            id: p.id,
            name: p.name,
            description: p.description,
            tracks: p.tracks.into_iter().map(Into::into).collect(),
            source: p.source.into(),
        }
    }
}

impl From<Playlist> for DomainPlaylist {
    fn from(p: Playlist) -> Self {
        Self {
            id: p.id,
            name: p.name,
            description: p.description,
            tracks: p.tracks.into_iter().map(Into::into).collect(),
            source: p.source.into(),
        }
    }
}
