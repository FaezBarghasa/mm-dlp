use url::Url;

#[derive(Debug, PartialEq, Eq)]
pub enum ExtractorError {
    InvalidUrl,
    UnsupportedPlatform,
    ExtractionFailed(String),
}

#[derive(Debug)]
pub struct MediaMetadata {
    pub platform: &'static str,
    pub media_type: String,
    pub media_id: String,
    pub url: String,
}

/// Trait defining a generalized media extractor for diverse platforms.
pub trait PlatformExtractor: Send + Sync {
    fn platform_name(&self) -> &'static str;
    fn can_handle(&self, url: &Url) -> bool;
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError>;
}

pub struct YouTubeExtractor;
impl PlatformExtractor for YouTubeExtractor {
    fn platform_name(&self) -> &'static str { "YouTube" }
    
    fn can_handle(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or("");
        host.contains("youtube.com") || host.contains("youtu.be")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let host = url.host_str().unwrap_or("");
        let media_id = if host.contains("youtu.be") {
            url.path().trim_matches('/').to_string()
        } else {
            url.query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
                .ok_or_else(|| ExtractorError::ExtractionFailed("No video ID found".to_string()))?
        };
        
        Ok(MediaMetadata {
            platform: self.platform_name(),
            media_type: "video".to_string(),
            media_id,
            url: url.to_string(),
        })
    }
}

pub struct InstagramExtractor;
impl PlatformExtractor for InstagramExtractor {
    fn platform_name(&self) -> &'static str { "Instagram" }
    
    fn can_handle(&self, url: &Url) -> bool {
        url.host_str().unwrap_or("").contains("instagram.com")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        for (i, segment) in segments.iter().enumerate() {
            if (*segment == "p" || *segment == "reel" || *segment == "tv") && i + 1 < segments.len() {
                return Ok(MediaMetadata {
                    platform: self.platform_name(),
                    media_type: segment.to_string(),
                    media_id: segments[i + 1].to_string(),
                    url: url.to_string(),
                });
            }
        }
        Err(ExtractorError::ExtractionFailed("No Instagram post/reel ID found".to_string()))
    }
}

pub struct TikTokExtractor;
impl PlatformExtractor for TikTokExtractor {
    fn platform_name(&self) -> &'static str { "TikTok" }
    
    fn can_handle(&self, url: &Url) -> bool {
        url.host_str().unwrap_or("").contains("tiktok.com")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        for (i, segment) in segments.iter().enumerate() {
            if *segment == "video" && i + 1 < segments.len() {
                return Ok(MediaMetadata {
                    platform: self.platform_name(),
                    media_type: "video".to_string(),
                    media_id: segments[i + 1].to_string(),
                    url: url.to_string(),
                });
            }
        }
        Err(ExtractorError::ExtractionFailed("No TikTok video ID found".to_string()))
    }
}

pub struct XExtractor;
impl PlatformExtractor for XExtractor {
    fn platform_name(&self) -> &'static str { "X" }
    
    fn can_handle(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or("");
        host.contains("x.com") || host.contains("twitter.com")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        for (i, segment) in segments.iter().enumerate() {
            if *segment == "status" && i + 1 < segments.len() {
                return Ok(MediaMetadata {
                    platform: self.platform_name(),
                    media_type: "status".to_string(),
                    media_id: segments[i + 1].to_string(),
                    url: url.to_string(),
                });
            }
        }
        Err(ExtractorError::ExtractionFailed("No X/Twitter status ID found".to_string()))
    }
}

pub struct SoundCloudExtractor;
impl PlatformExtractor for SoundCloudExtractor {
    fn platform_name(&self) -> &'static str { "SoundCloud" }
    
    fn can_handle(&self, url: &Url) -> bool {
        url.host_str().unwrap_or("").contains("soundcloud.com")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        if segments.len() >= 2 {
            let user = segments[0];
            let track = segments[1];
            return Ok(MediaMetadata {
                platform: self.platform_name(),
                media_type: "track".to_string(),
                media_id: format!("{}/{}", user, track),
                url: url.to_string(),
            });
        }
        Err(ExtractorError::ExtractionFailed("No SoundCloud track ID found".to_string()))
    }
}

pub struct SpotifyExtractor;
impl PlatformExtractor for SpotifyExtractor {
    fn platform_name(&self) -> &'static str { "Spotify" }
    
    fn can_handle(&self, url: &Url) -> bool {
        url.host_str().unwrap_or("").contains("spotify.com")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        for (i, segment) in segments.iter().enumerate() {
            if (*segment == "track" || *segment == "album" || *segment == "playlist" || *segment == "episode") && i + 1 < segments.len() {
                return Ok(MediaMetadata {
                    platform: self.platform_name(),
                    media_type: segment.to_string(),
                    media_id: segments[i + 1].to_string(),
                    url: url.to_string(),
                });
            }
        }
        Err(ExtractorError::ExtractionFailed("No Spotify ID found".to_string()))
    }
}

pub struct AppleMusicExtractor;
impl PlatformExtractor for AppleMusicExtractor {
    fn platform_name(&self) -> &'static str { "Apple Music" }
    
    fn can_handle(&self, url: &Url) -> bool {
        url.host_str().unwrap_or("").contains("music.apple.com")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "i") {
            return Ok(MediaMetadata {
                platform: self.platform_name(),
                media_type: "track".to_string(),
                media_id: v.into_owned(),
                url: url.to_string(),
            });
        }
        
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        for (i, segment) in segments.iter().enumerate() {
            if (*segment == "album" || *segment == "playlist") && i + 1 < segments.len() {
                return Ok(MediaMetadata {
                    platform: self.platform_name(),
                    media_type: segment.to_string(),
                    media_id: segments.last().unwrap_or(&"").to_string(),
                    url: url.to_string(),
                });
            }
        }
        
        Err(ExtractorError::ExtractionFailed("No Apple Music ID found".to_string()))
    }
}

pub struct TwitchExtractor;
impl PlatformExtractor for TwitchExtractor {
    fn platform_name(&self) -> &'static str { "Twitch" }
    
    fn can_handle(&self, url: &Url) -> bool {
        url.host_str().unwrap_or("").contains("twitch.tv")
    }
    
    fn extract_metadata(&self, url: &Url) -> Result<MediaMetadata, ExtractorError> {
        let segments: Vec<&str> = url.path_segments().map(|c| c.collect()).unwrap_or_default();
        for (i, segment) in segments.iter().enumerate() {
            if *segment == "videos" && i + 1 < segments.len() {
                return Ok(MediaMetadata {
                    platform: self.platform_name(),
                    media_type: "video".to_string(),
                    media_id: segments[i + 1].to_string(),
                    url: url.to_string(),
                });
            }
        }
        
        if !segments.is_empty() && !segments[0].is_empty() {
            return Ok(MediaMetadata {
                platform: self.platform_name(),
                media_type: "channel".to_string(),
                media_id: segments[0].to_string(),
                url: url.to_string(),
            });
        }
        
        Err(ExtractorError::ExtractionFailed("No Twitch ID/Channel found".to_string()))
    }
}

/// Core registry routing URLs to the correct platform extractor
pub struct PlatformRegistry {
    extractors: Vec<Box<dyn PlatformExtractor>>,
}

impl PlatformRegistry {
    pub fn new() -> Self {
        Self {
            extractors: vec![
                Box::new(YouTubeExtractor),
                Box::new(InstagramExtractor),
                Box::new(TikTokExtractor),
                Box::new(XExtractor),
                Box::new(SoundCloudExtractor),
                Box::new(SpotifyExtractor),
                Box::new(AppleMusicExtractor),
                Box::new(TwitchExtractor),
            ],
        }
    }

    pub fn extract(&self, raw_url: &str) -> Result<MediaMetadata, ExtractorError> {
        let parsed_url = Url::parse(raw_url).map_err(|_| ExtractorError::InvalidUrl)?;
        
        for extractor in &self.extractors {
            if extractor.can_handle(&parsed_url) {
                return extractor.extract_metadata(&parsed_url);
            }
        }
        
        Err(ExtractorError::UnsupportedPlatform)
    }
}

impl Default for PlatformRegistry {
    fn default() -> Self {
        Self::new()
    }
}