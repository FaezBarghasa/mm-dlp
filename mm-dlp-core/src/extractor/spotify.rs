use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::PlatformExtractor;
use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;

pub struct SpotifyExtractor;

impl SpotifyExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpotifyExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PlatformExtractor for SpotifyExtractor {
    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaMetadata, EngineError> {
        let oembed_url = format!("https://open.spotify.com/oembed?url={}", urlencoding::encode(url));
        let resp = client.get(&oembed_url).send().await?;
        
        let title_fallback;
        let thumbnail_url;
        if resp.status().is_success() {
            let json: Value = resp.json().await.unwrap_or(Value::Null);
            title_fallback = json["title"].as_str().unwrap_or("Unknown Title").to_string();
            thumbnail_url = json["thumbnail_url"].as_str().map(String::from);
        } else {
            title_fallback = "Unknown Title".to_string();
            thumbnail_url = None;
        }

        let page_html = client.get(url).send().await?.text().await.unwrap_or_default();
        let mut title = title_fallback;
        let mut artist = "Unknown Artist".to_string();
        let mut album = None;
        let mut duration_seconds = 0u64;

        if let Some(start_idx) = page_html.find("<script type=\"application/ld+json\">") {
            let rest = &page_html[start_idx + "<script type=\"application/ld+json\">".len()..];
            if let Some(end_idx) = rest.find("</script>") {
                let json_str = &rest[..end_idx];
                if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                    if let Some(t) = val["name"].as_str() {
                        title = t.to_string();
                    }
                    if let Some(by_artist) = val["byArtist"].as_array() {
                        let artists: Vec<String> = by_artist
                            .iter()
                            .filter_map(|a| a["name"].as_str().map(String::from))
                            .collect();
                        if !artists.is_empty() {
                            artist = artists.join(", ");
                        }
                    } else if let Some(a) = val["byArtist"]["name"].as_str() {
                        artist = a.to_string();
                    }
                    if let Some(alb) = val["inAlbum"]["name"].as_str() {
                        album = Some(alb.to_string());
                    }
                    if let Some(dur_str) = val["duration"].as_str() {
                        // Parses ISO 8601 duration string like PT3M15S
                        let dur = dur_str
                            .trim_start_matches("PT")
                            .trim_end_matches('S');
                        if let Some((m, s)) = dur.split_once('M') {
                            let mins: u64 = m.parse().unwrap_or(0);
                            let secs: u64 = s.parse().unwrap_or(0);
                            duration_seconds = mins * 60 + secs;
                        }
                    }
                }
            }
        }

        let track_id = url
            .split("/track/")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .unwrap_or("spotify_track")
            .to_string();

        Ok(MediaMetadata {
            id: track_id,
            title,
            artist,
            album,
            duration_seconds,
            thumbnail_url,
            webpage_url: url.to_string(),
        })
    }

    async fn search(&self, _client: &Client, _query: &str) -> Result<Vec<MediaMetadata>, EngineError> {
        Err(EngineError::StreamNotFound(
            "Spotify search is not supported directly; Spotify is metadata-only.".to_string(),
        ))
    }

    async fn get_stream_url(&self, _client: &Client, _track_id: &str) -> Result<StreamCandidate, EngineError> {
        Err(EngineError::StreamNotFound(
            "Spotify tracks are DRM protected. Stream must be resolved from YouTube/SoundCloud.".to_string(),
        ))
    }
}
