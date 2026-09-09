use std::sync::Arc;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;

use crate::client::EngineError;
use crate::extractor::traits::{AsyncExtractor, MediaFormat, MediaInfo};

// ==========================================
// Concrete Extractor Implementations
// ==========================================

pub struct YouTubeExtractor {
    pattern: Regex,
}

impl YouTubeExtractor {
    pub fn new() -> Self {
        Self {
            pattern: Regex::new(r"^(?:https?://)?(?:www\.)?(?:youtube\.com/watch\?v=|youtu\.be/)([a-zA-Z0-9_-]{11})").expect("Invalid YouTube Regex"),
        }
    }
}

#[async_trait]
impl AsyncExtractor for YouTubeExtractor {
    fn matches_url(&self, url: &str) -> bool {
        self.pattern.is_match(url)
    }

    async fn extract_metadata(&self, _client: &Client, url: &str) -> Result<MediaInfo, EngineError> {
        let captures = self.pattern.captures(url).ok_or_else(|| {
            EngineError::OsApiError("Failed to extract YouTube Video ID from matching URL".into())
        })?;
        
        let video_id = captures.get(1).map_or("", |m| m.as_str()).to_string();

        Ok(MediaInfo {
            id: video_id.clone(),
            title: format!("YouTube Video {}", video_id),
            description: Some("Video description placeholder".into()),
            uploader: Some("YouTube Uploader".into()),
            duration: Some(240),
            formats: vec![
                MediaFormat {
                    format_id: "137".into(),
                    url: format!("https://rr1.sn-ab5l.googlevideo.com/videoplayback?id={}", video_id),
                    ext: "mp4".into(),
                    width: Some(1920),
                    height: Some(1080),
                    vcodec: Some("avc1.640028".into()),
                    acodec: Some("mp4a.40.2".into()),
                    filesize: Some(15000000),
                }
            ]
        })
    }
}

pub struct VimeoExtractor {
    pattern: Regex,
}

impl VimeoExtractor {
    pub fn new() -> Self {
        Self {
            pattern: Regex::new(r"^(?:https?://)?(?:www\.)?vimeo\.com/(\d+)").expect("Invalid Vimeo Regex"),
        }
    }
}

#[async_trait]
impl AsyncExtractor for VimeoExtractor {
    fn matches_url(&self, url: &str) -> bool {
        self.pattern.is_match(url)
    }

    async fn extract_metadata(&self, _client: &Client, _url: &str) -> Result<MediaInfo, EngineError> {
        Err(EngineError::OsApiError("Vimeo extraction logic not yet hooked up to client".into()))
    }
}

// ==========================================
// Static Routing Registry
// ==========================================

pub static EXTRACTOR_REGISTRY: Lazy<Vec<Arc<dyn AsyncExtractor>>> = Lazy::new(|| {
    vec![
        Arc::new(YouTubeExtractor::new()),
        Arc::new(VimeoExtractor::new()),
    ]
});

pub fn route_url(url: &str) -> Option<Arc<dyn AsyncExtractor>> {
    EXTRACTOR_REGISTRY.iter().find(|&extractor| extractor.matches_url(url)).cloned()
}