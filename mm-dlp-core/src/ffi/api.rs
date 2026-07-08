use crate::data::router::StreamRouter;
use crate::data::playlist::{json_handler, xml_handler};
use crate::domain::playlist::Playlist as DomainPlaylist;
use crate::ffi::types::{AudioSource, AudioQuality, Playlist, TrackMetadata};
use crate::media::pipeline::process_downloaded_file;
use crate::media::converter::AudioFormat;
use anyhow::Result;
use std::panic::catch_unwind;
use std::path::Path;
use uniffi::export;

pub struct MmDlpApi {
    router: StreamRouter,
}

impl MmDlpApi {
    pub fn new() -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let router = rt.block_on(StreamRouter::new())?;
        Ok(Self { router })
    }
}

#[export]
impl MmDlpApi {
    pub fn search(&self, query: String, source: AudioSource) -> Result<Vec<TrackMetadata>> {
        let result = catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                // This is a placeholder for the actual search implementation
                Ok(vec![])
            })
        });
        result.unwrap_or_else(|_| Err(anyhow::anyhow!("Search panicked")))
    }

    pub fn download_track(&self, url: String, quality: AudioQuality, format: AudioFormat, temp_dir: String) -> Result<String> {
        let result = catch_unwind(|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                // This is a placeholder for the actual download implementation
                let temp_path = Path::new(&temp_dir).join("temp_file");
                tokio::fs::write(&temp_path, "dummy content").await.unwrap();
                Ok(temp_path.to_str().unwrap().to_string())
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
