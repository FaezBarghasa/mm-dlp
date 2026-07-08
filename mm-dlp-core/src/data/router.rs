use crate::domain::models::{AudioSource, AudioQuality, StreamInfo};
use crate::domain::extractor::PlatformExtractor;
use crate::data::extractors::youtube::YouTubeMusicExtractor;
use crate::data::extractors::soundcloud::SoundCloudExtractor;
use anyhow::{anyhow, Result};
use std::sync::Arc;

pub struct StreamRouter {
    youtube_extractor: Arc<YouTubeMusicExtractor>,
    soundcloud_extractor: Arc<SoundCloudExtractor>,
}

impl StreamRouter {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            youtube_extractor: Arc::new(YouTubeMusicExtractor::new()?),
            soundcloud_extractor: Arc::new(SoundCloudExtractor::new().await?),
        })
    }

    pub async fn get_stream(&self, source: &AudioSource, track_id: &str, quality: AudioQuality) -> Result<StreamInfo> {
        match source {
            AudioSource::YouTubeMusic => self.youtube_extractor.get_stream_url(track_id, quality).await,
            AudioSource::SoundCloud => self.soundcloud_extractor.get_stream_url(track_id, quality).await,
            AudioSource::Spotify => Err(anyhow!("Spotify streaming is not supported, metadata only.")),
        }
    }
}
