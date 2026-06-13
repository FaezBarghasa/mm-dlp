use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFormat {
    pub format_id: String,
    pub url: String,
    pub extension: String,
    pub resolution: Option<String>,
    pub file_size: Option<u64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub is_dash_fragmented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub title: String,
    pub description: String,
    pub uploader: String,
    pub duration_seconds: Option<f64>,
    pub formats: Vec<MediaFormat>,
}

#[async_trait]
pub trait AsyncExtractor: Send + Sync {
    fn matches_url(&self, url: &str) -> bool;
    async fn extract_metadata(&self, client: &reqwest::Client, url: &str) -> Result<MediaInfo>;
}
