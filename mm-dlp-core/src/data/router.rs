use crate::extractor::traits::{AudioSource, AudioQuality, StreamInfo, TrackMetadata, AudioPlatformExtractor};
use crate::data::extractors::youtube::YouTubeMusicExtractor;
use crate::data::extractors::soundcloud::SoundCloudExtractor;
use anyhow::{anyhow, Result};
use std::sync::Arc;

/// Routes search and stream-URL resolution to the correct platform extractor.
pub struct StreamRouter {
    pub(crate) youtube_extractor: Arc<YouTubeMusicExtractor>,
    pub(crate) soundcloud_extractor: Arc<SoundCloudExtractor>,
}

impl StreamRouter {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            youtube_extractor: Arc::new(YouTubeMusicExtractor::new()?),
            soundcloud_extractor: Arc::new(SoundCloudExtractor::new().await?),
        })
    }

    /// Searches the given platform for tracks matching `query`.
    pub async fn search(&self, query: &str, source: &AudioSource) -> Result<Vec<TrackMetadata>> {
        match source {
            AudioSource::YouTubeMusic => self.youtube_extractor.search(query).await,
            AudioSource::SoundCloud => self.soundcloud_extractor.search(query).await,
            AudioSource::Spotify => Err(anyhow!("Spotify search requires authentication; metadata-only mode.")),
        }
    }

    /// Resolves the audio stream URL for a track on the given platform.
    pub async fn get_stream(
        &self,
        source: &AudioSource,
        track_id: &str,
        quality: AudioQuality,
    ) -> Result<StreamInfo> {
        match source {
            AudioSource::YouTubeMusic => {
                self.youtube_extractor.get_stream_url(track_id, quality).await
            }
            AudioSource::SoundCloud => {
                self.soundcloud_extractor.get_stream_url(track_id, quality).await
            }
            AudioSource::Spotify => {
                Err(anyhow!("Spotify streaming is not supported; metadata only."))
            }
        }
    }
}
