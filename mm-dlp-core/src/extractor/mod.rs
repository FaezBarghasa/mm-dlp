use async_trait::async_trait;
use reqwest::Client;
use std::sync::Arc;
use std::collections::HashMap;

use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;
use crate::utils;

pub mod spotify;
pub mod soundcloud;
pub mod youtube;

/// Trait implemented by all platform-specific extractors.
#[async_trait]
pub trait PlatformExtractor: Send + Sync {
    /// Extract metadata for a given track URL.
    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaMetadata, EngineError>;

    /// Search for tracks on the platform by query.
    async fn search(&self, client: &Client, query: &str) -> Result<Vec<MediaMetadata>, EngineError>;

    /// Retrieve direct audio stream candidate for a track.
    async fn get_stream_url(&self, client: &Client, track_id: &str) -> Result<StreamCandidate, EngineError>;
}

/// Registry holding extractors for supported platforms.
pub struct PlatformRegistry {
    extractors: HashMap<&'static str, Arc<dyn PlatformExtractor>>,
}

impl PlatformRegistry {
    pub fn new() -> Self {
        let mut extractors: HashMap<&'static str, Arc<dyn PlatformExtractor>> = HashMap::new();
        extractors.insert("spotify", Arc::new(spotify::SpotifyExtractor::new()));
        extractors.insert("soundcloud", Arc::new(soundcloud::SoundCloudExtractor::new()));
        extractors.insert("youtube", Arc::new(youtube::YouTubeExtractor::new()));
        Self { extractors }
    }

    pub fn get_extractor(&self, platform: &str) -> Option<Arc<dyn PlatformExtractor>> {
        self.extractors.get(platform).cloned()
    }

    pub fn route_url(&self, url: &str) -> Option<Arc<dyn PlatformExtractor>> {
        let platform = utils::detect_platform(url)?;
        self.get_extractor(platform)
    }
}

impl Default for PlatformRegistry {
    fn default() -> Self {
        Self::new()
    }
}