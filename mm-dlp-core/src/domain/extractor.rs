use async_trait::async_trait;
use crate::domain::models::{AudioQuality, StreamInfo, TrackMetadata};
use anyhow::Result;

#[async_trait]
pub trait PlatformExtractor: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>>;
    async fn get_stream_url(&self, track_id: &str, quality: AudioQuality) -> Result<StreamInfo>;
}