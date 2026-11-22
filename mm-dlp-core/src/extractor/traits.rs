use crate::error::EngineError;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    pub ext: String,
    pub url: Option<String>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaFormat {
    pub format_id: String,
    pub url: String,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<u64>,
    pub tbr: Option<f64>,
    pub protocol: Option<String>,
    pub http_headers: Option<HashMap<String, String>>,
}

impl MediaFormat {
    pub fn is_audio_only(&self) -> bool {
        self.vcodec.as_deref() == Some("none")
            || (self.vcodec.is_none()
                && self.acodec.is_some()
                && self.acodec.as_deref() != Some("none"))
    }

    pub fn is_video_only(&self) -> bool {
        self.acodec.as_deref() == Some("none")
            || (self.acodec.is_none()
                && self.vcodec.is_some()
                && self.vcodec.as_deref() != Some("none"))
    }

    pub fn has_video(&self) -> bool {
        self.vcodec.is_some() && self.vcodec.as_deref() != Some("none")
    }

    pub fn has_audio(&self) -> bool {
        self.acodec.is_some() && self.acodec.as_deref() != Some("none")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<u32>,
    pub thumbnail: Option<String>,
    pub webpage_url: Option<String>,
    pub formats: Vec<MediaFormat>,
    pub subtitles: Option<HashMap<String, Vec<Subtitle>>>,
}

impl MediaInfo {
    pub fn best_audio(&self) -> Option<&MediaFormat> {
        self.formats
            .iter()
            .filter(|f| f.has_audio())
            .max_by(|a, b| {
                let a_tbr = a.tbr.unwrap_or(0.0);
                let b_tbr = b.tbr.unwrap_or(0.0);
                a_tbr
                    .partial_cmp(&b_tbr)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    pub fn best_video(&self) -> Option<&MediaFormat> {
        self.formats
            .iter()
            .filter(|f| f.has_video())
            .max_by_key(|f| {
                (
                    f.height.unwrap_or(0),
                    f.width.unwrap_or(0),
                    (f.tbr.unwrap_or(0.0) * 1000.0) as u64,
                )
            })
    }

    pub fn best_format(&self) -> Option<&MediaFormat> {
        self.best_video()
            .or_else(|| self.best_audio())
            .or_else(|| self.formats.first())
    }
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
    /// Canonical short name for the extractor (e.g. "youtube", "vimeo").
    fn name(&self) -> &'static str;

    /// Uses zero-copy regex evaluation to determine if the URL is supported.
    fn matches_url(&self, url: &str) -> bool;

    /// Extracts metadata and applicable formats based on the provided target platform URL.
    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaInfo, EngineError>;
}

#[async_trait]
pub trait AudioPlatformExtractor: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>>;
    async fn get_stream_url(&self, track_id: &str, quality: AudioQuality) -> Result<StreamInfo>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_selection() {
        let info = MediaInfo {
            id: "vid123".into(),
            title: "Test Video".into(),
            description: None,
            uploader: None,
            duration: Some(120),
            thumbnail: None,
            webpage_url: None,
            formats: vec![
                MediaFormat {
                    format_id: "audio_only".into(),
                    url: "http://audio.mp4".into(),
                    ext: "m4a".into(),
                    width: None,
                    height: None,
                    fps: None,
                    vcodec: Some("none".into()),
                    acodec: Some("mp4a.40.2".into()),
                    filesize: Some(1000),
                    tbr: Some(128.0),
                    protocol: None,
                    http_headers: None,
                },
                MediaFormat {
                    format_id: "video_720p".into(),
                    url: "http://720p.mp4".into(),
                    ext: "mp4".into(),
                    width: Some(1280),
                    height: Some(720),
                    fps: Some(30.0),
                    vcodec: Some("avc1".into()),
                    acodec: Some("mp4a".into()),
                    filesize: Some(5000),
                    tbr: Some(1500.0),
                    protocol: None,
                    http_headers: None,
                },
                MediaFormat {
                    format_id: "video_1080p".into(),
                    url: "http://1080p.mp4".into(),
                    ext: "mp4".into(),
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(60.0),
                    vcodec: Some("avc1".into()),
                    acodec: Some("mp4a".into()),
                    filesize: Some(12000),
                    tbr: Some(4000.0),
                    protocol: None,
                    http_headers: None,
                },
            ],
            subtitles: None,
        };

        assert_eq!(info.best_video().unwrap().format_id, "video_1080p");
        assert_eq!(info.best_format().unwrap().format_id, "video_1080p");
        assert_eq!(info.formats[0].is_audio_only(), true);
        assert_eq!(info.formats[1].has_video(), true);
    }
}
