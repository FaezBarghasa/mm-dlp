/// Represents the metadata extracted from a supported media URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaMetadata {
    /// The name of the platform (e.g., "YouTube", "Twitter").
    pub platform: String,
    /// The type of media content (e.g., "Video", "Post", "Track").
    pub media_type: String,
    /// The unique identifier for the media on its respective platform.
    pub media_id: String,
}

/// Core trait that allows scaling to an arbitrary number of media platforms.
/// Any new platform extractor must implement this trait.
pub trait PlatformExtractor: Send + Sync {
    /// Attempts to extract media metadata from the given URL.
    /// Returns `Some(MediaMetadata)` if successful, or `None` if the URL is not recognized by this extractor.
    fn extract(&self, url: &str) -> Option<MediaMetadata>;
}

/// Safely strips query parameters, fragment identifiers, and trailing slashes 
/// from parsed URL segments to reliably isolate the raw ID.
fn clean_id(part: &str) -> String {
    part.split('?')
        .next()
        .unwrap_or("")
        .split('&')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// A registry that holds all available platform extractors.
/// It iterates through them to find the appropriate one for a given URL.
pub struct PlatformRegistry {
    /// A collection of boxed, dynamic extractors implementing `PlatformExtractor`.
    extractors: Vec<Box<dyn PlatformExtractor>>,
}

impl Default for PlatformRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformRegistry {
    /// Creates a new `PlatformRegistry` pre-populated with all supported platform extractors.
    pub fn new() -> Self {
        Self {
            extractors: vec![
                Box::new(YouTubeExtractor),
                Box::new(TwitterExtractor),
                Box::new(InstagramExtractor),
                Box::new(TikTokExtractor),
                Box::new(RedditExtractor),
                Box::new(TwitchExtractor),
                Box::new(SpotifyExtractor),
                Box::new(VimeoExtractor),
                Box::new(SoundCloudExtractor),
            ],
        }
    }

    /// Attempts to extract media metadata from the given URL using the registered extractors.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL string to parse.
    ///
    /// # Returns
    ///
    /// * `Ok(MediaMetadata)` if an extractor successfully parses the URL.
    /// * `Err(String)` if no supported platform matches the URL.
    pub fn extract(&self, url: &str) -> Result<MediaMetadata, String> {
        for extractor in &self.extractors {
            if let Some(metadata) = extractor.extract(url) {
                return Ok(metadata);
            }
        }
        Err(format!("No supported platform found for URL: {}", url))
    }
}

// ==========================================
// Platform-Specific Extractor Implementations
// ==========================================

/// Extractor for YouTube URLs (videos and shorts).
struct YouTubeExtractor;
impl PlatformExtractor for YouTubeExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        // Standard YouTube video URL
        if url.contains("youtube.com/watch") {
            if let Some(v_idx) = url.find("v=") {
                let id = clean_id(&url[v_idx + 2..]);
                if !id.is_empty() {
                    return Some(MediaMetadata { platform: "YouTube".to_string(), media_type: "Video".to_string(), media_id: id });
                }
            }
        // Shortened youtu.be URL
        } else if let Some(be_idx) = url.find("youtu.be/") {
            let id = clean_id(&url[be_idx + 9..]);
            if !id.is_empty() {
                return Some(MediaMetadata { platform: "YouTube".to_string(), media_type: "Video".to_string(), media_id: id });
            }
        // YouTube Shorts URL
        } else if let Some(short_idx) = url.find("youtube.com/shorts/") {
            let id = clean_id(&url[short_idx + 19..]);
            if !id.is_empty() {
                return Some(MediaMetadata { platform: "YouTube".to_string(), media_type: "Short".to_string(), media_id: id });
            }
        }
        None
    }
}

/// Extractor for Twitter/X URLs.
struct TwitterExtractor;
impl PlatformExtractor for TwitterExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        // Match both legacy twitter.com and new x.com domains
        if url.contains("twitter.com/") || url.contains("x.com/") {
            if let Some(status_idx) = url.find("/status/") {
                let id = clean_id(&url[status_idx + 8..]);
                if !id.is_empty() {
                    return Some(MediaMetadata { platform: "Twitter".to_string(), media_type: "Post".to_string(), media_id: id });
                }
            }
        }
        None
    }
}

/// Extractor for Instagram URLs (posts, reels, IGTV).
struct InstagramExtractor;
impl PlatformExtractor for InstagramExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("instagram.com/") {
            let markers = [("/p/", "Post"), ("/reel/", "Reel"), ("/tv/", "IGTV")];
            for (marker, m_type) in markers {
                if let Some(idx) = url.find(marker) {
                    let id = clean_id(&url[idx + marker.len()..]);
                    if !id.is_empty() {
                        return Some(MediaMetadata { platform: "Instagram".to_string(), media_type: m_type.to_string(), media_id: id });
                    }
                }
            }
        }
        None
    }
}

/// Extractor for TikTok URLs.
struct TikTokExtractor;
impl PlatformExtractor for TikTokExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("tiktok.com/") {
            if let Some(video_idx) = url.find("/video/") {
                let id = clean_id(&url[video_idx + 7..]);
                if !id.is_empty() {
                    return Some(MediaMetadata { platform: "TikTok".to_string(), media_type: "Video".to_string(), media_id: id });
                }
            }
        }
        None
    }
}

/// Extractor for Reddit URLs.
struct RedditExtractor;
impl PlatformExtractor for RedditExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        // Look for the standard Reddit comment/post URL pattern
        if url.contains("reddit.com/r/") {
            if let Some(comments_idx) = url.find("/comments/") {
                let id = clean_id(&url[comments_idx + 10..]);
                if !id.is_empty() {
                    return Some(MediaMetadata { platform: "Reddit".to_string(), media_type: "Post".to_string(), media_id: id });
                }
            }
        }
        None
    }
}

/// Extractor for Twitch URLs (VODs, clips, streams).
struct TwitchExtractor;
impl PlatformExtractor for TwitchExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("twitch.tv/") {
            // VOD
            if let Some(videos_idx) = url.find("/videos/") {
                let id = clean_id(&url[videos_idx + 8..]);
                if !id.is_empty() { return Some(MediaMetadata { platform: "Twitch".to_string(), media_type: "Video".to_string(), media_id: id }); }
            // Clip within channel
            } else if let Some(clip_idx) = url.find("clip/") {
                let id = clean_id(&url[clip_idx + 5..]);
                if !id.is_empty() { return Some(MediaMetadata { platform: "Twitch".to_string(), media_type: "Clip".to_string(), media_id: id }); }
            // Dedicated clips domain
            } else if url.contains("clips.twitch.tv/") {
                let id_part = url.split("clips.twitch.tv/").nth(1).unwrap_or("");
                let id = clean_id(id_part);
                if !id.is_empty() { return Some(MediaMetadata { platform: "Twitch".to_string(), media_type: "Clip".to_string(), media_id: id }); }
            // Live stream (channel name)
            } else {
                let host_idx = url.find("twitch.tv/").unwrap();
                let channel = clean_id(&url[host_idx + 10..]);
                if !channel.is_empty() { return Some(MediaMetadata { platform: "Twitch".to_string(), media_type: "Stream".to_string(), media_id: channel }); }
            }
        }
        None
    }
}

/// Extractor for Spotify URLs (tracks, albums, playlists, episodes).
struct SpotifyExtractor;
impl PlatformExtractor for SpotifyExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("spotify.com/") {
            let markers = [("/track/", "Track"), ("/album/", "Album"), ("/playlist/", "Playlist"), ("/episode/", "Episode")];
            for (marker, m_type) in markers {
                if let Some(idx) = url.find(marker) {
                    let id = clean_id(&url[idx + marker.len()..]);
                    if !id.is_empty() { return Some(MediaMetadata { platform: "Spotify".to_string(), media_type: m_type.to_string(), media_id: id }); }
                }
            }
        }
        None
    }
}

/// Extractor for Vimeo URLs.
struct VimeoExtractor;
impl PlatformExtractor for VimeoExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("vimeo.com/") {
            let host_idx = url.find("vimeo.com/").unwrap();
            let id = clean_id(&url[host_idx + 10..]);
            // Ensure the extracted ID is numeric, as Vimeo video IDs are numbers
            if !id.is_empty() && id.chars().all(char::is_numeric) {
                return Some(MediaMetadata { platform: "Vimeo".to_string(), media_type: "Video".to_string(), media_id: id });
            }
        }
        None
    }
}

/// Extractor for SoundCloud URLs.
struct SoundCloudExtractor;
impl PlatformExtractor for SoundCloudExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("soundcloud.com/") {
            let host_idx = url.find("soundcloud.com/").unwrap();
            let path = &url[host_idx + 15..];
            let parts: Vec<&str> = path.split('?').next().unwrap_or("").split('#').next().unwrap_or("").split('/').collect();
            
            // Expected format: soundcloud.com/{artist}/{track}
            if parts.len() >= 2 {
                let artist = parts[0];
                let track = parts[1];
                if !artist.is_empty() && !track.is_empty() {
                    return Some(MediaMetadata {
                        platform: "SoundCloud".to_string(),
                        media_type: "Track".to_string(),
                        // Combine artist and track as the unique identifier
                        media_id: format!("{}/{}", artist, track),
                    });
                }
            }
        }
        None
    }
}