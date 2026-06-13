//! # mm-dlp-core
//!
//! This crate provides the core functionality for `mm-dlp`.

// This sets up the uniffi scaffolding.
uniffi::setup_scaffolding!();

// Export all modules
pub mod error;
pub mod client;
pub mod downloader;
pub mod extractor;
pub mod js;
pub mod plugin;
pub mod postprocessor;

// Re-export all necessary types for UniFFI
pub use crate::error::EngineError;
pub use crate::extractor::traits::{MediaFormat, MediaInfo};
pub use crate::postprocessor::ffmpeg::FfmpegProgress;

/// Maps Rust-native async behaviors into cross-platform compatible delegates
pub trait DownloadProgressCallback: Send + Sync {
    fn on_progress(&self, progress: FfmpegProgress);
    fn on_complete(&self);
    fn on_error(&self, error: EngineError);
}

pub struct MmDlpEngine {
    client: reqwest::Client,
}

impl Default for MmDlpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MmDlpEngine {
    pub fn new() -> Self {
        // Build a standard reqwest client.
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub fn extract_metadata(&self, url: String) -> Result<MediaInfo, EngineError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| EngineError::OsApiError(e.to_string()))?;

        rt.block_on(async {
            let extractor = crate::extractor::registry::route_url(&url)
                .ok_or_else(|| EngineError::OsApiError(format!("No extractor matched URL: {}", url)))?;
            
            extractor.extract_metadata(&self.client, &url).await
        })
    }

    pub fn download_and_mux(
        &self,
        video_url: String,
        audio_url: String,
        output_path: String,
        callback: Box<dyn DownloadProgressCallback>,
    ) -> Result<(), EngineError> {
        let callback_arc: std::sync::Arc<dyn DownloadProgressCallback> = std::sync::Arc::from(callback);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| EngineError::OsApiError(e.to_string()))?;

        rt.block_on(async {
            let temp_dir = std::env::temp_dir();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let video_tmp = temp_dir.join(format!("video_temp_{}.mp4", timestamp));
            let audio_tmp = temp_dir.join(format!("audio_temp_{}.m4a", timestamp));

            // Helper to download a single/manifest media file
            async fn download_media_file(
                client: &reqwest::Client,
                url: &str,
                output_path: &std::path::Path,
            ) -> Result<(), EngineError> {
                let mut segments = Vec::new();
                
                // Simple manifest detection
                if url.contains(".m3u8") {
                    let manifest_content = client.get(url).send().await
                        .map_err(|e| EngineError::Network(e.to_string()))?
                        .text().await
                        .map_err(|e| EngineError::Network(e.to_string()))?;
                    let base_url = if let Some(last_slash) = url.rfind('/') {
                        &url[..last_slash]
                    } else {
                        url
                    };
                    segments = crate::downloader::manifest::parse_m3u8(&manifest_content, base_url)?;
                } else if url.contains(".mpd") {
                    let manifest_content = client.get(url).send().await
                        .map_err(|e| EngineError::Network(e.to_string()))?
                        .text().await
                        .map_err(|e| EngineError::Network(e.to_string()))?;
                    let base_url = if let Some(last_slash) = url.rfind('/') {
                        &url[..last_slash]
                    } else {
                        url
                    };
                    segments = crate::downloader::manifest::parse_dash(&manifest_content, base_url)?;
                } else {
                    segments.push(crate::downloader::manifest::DownloadSegment {
                        index: 0,
                        url: url.to_string(),
                    });
                }

                let total_segments = segments.len();
                if total_segments == 0 {
                    return Err(EngineError::OsApiError("No download segments found".into()));
                }

                let (tx, rx) = tokio::sync::mpsc::channel(total_segments.max(1));
                
                // Spawn parallel downloader
                let client_clone = client.clone();
                let download_handle = tokio::spawn(crate::downloader::parallel::download_segments(
                    client_clone,
                    segments,
                    4, // concurrency limit
                    tx,
                ));

                // Flush to disk sequentially
                let flusher = crate::downloader::flusher::SequentialFlusher::new();
                flusher.flush_to_disk(output_path, rx, total_segments).await?;

                download_handle.await.map_err(|e| EngineError::OsApiError(format!("Download task failed to join: {}", e)))?;
                Ok(())
            }

            // Download video
            let cb_video = callback_arc.clone();
            if let Err(e) = download_media_file(&self.client, &video_url, &video_tmp).await {
                cb_video.on_error(e.clone());
                return Err(e);
            }

            // Download audio
            let cb_audio = callback_arc.clone();
            if let Err(e) = download_media_file(&self.client, &audio_url, &audio_tmp).await {
                let _ = std::fs::remove_file(&video_tmp);
                cb_audio.on_error(e.clone());
                return Err(e);
            }

            // Mux them
            let muxer = crate::postprocessor::ffmpeg::FfmpegMuxer::new();
            let cb_mux = callback_arc.clone();
            let mux_result = muxer.mux_video_audio(
                &video_tmp,
                &audio_tmp,
                &output_path,
                move |progress| {
                    cb_mux.on_progress(progress);
                }
            ).await;

            // Cleanup temp files
            let _ = std::fs::remove_file(&video_tmp);
            let _ = std::fs::remove_file(&audio_tmp);

            match mux_result {
                Ok(_) => {
                    callback_arc.on_complete();
                    Ok(())
                }
                Err(e) => {
                    callback_arc.on_error(e.clone());
                    Err(e)
                }
            }
        })
    }
}