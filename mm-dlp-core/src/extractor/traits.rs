use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use crate::client::EngineError;

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