use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use crate::error::EngineError;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFormat {
    pub format_id: String,
    pub url: String,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<u32>,
    pub formats: Vec<MediaFormat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioSource {
    YouTubeMusic,
    SoundCloud,
    Spotify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioQuality {
    Low,
    Medium,
    High,
    Lossless,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_art_url: Option<String>,
    pub track_id: String,
    pub source: AudioSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub stream_url: String,
    pub format: String,
    pub bitrate: u32,
    pub duration_secs: u64,
    pub metadata: TrackMetadata,
}

/// Unified Async Extractor interface.
/// Enforces object-safety for dynamic dispatch and ensures the trait implementation
/// is thread-safe (`Send + Sync`) for our concurrent pipeline.
#[async_trait]
pub trait AsyncExtractor: Send + Sync {
    /// Uses zero-copy regex evaluation to determine if the URL is supported.
    fn matches_url(&self, url: &str) -> bool;
    
    /// Extacts metadata and applicable formats based on the provided target platform URL.
    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaInfo, EngineError>;
}

#[async_trait]
pub trait AudioPlatformExtractor: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>>;
    async fn get_stream_url(&self, track_id: &str, quality: AudioQuality) -> Result<StreamInfo>;
}
