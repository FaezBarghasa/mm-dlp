use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::sync::RwLock;

use super::PlatformExtractor;
use super::common::{clean_html, traverse_string};
use super::traits::{AsyncExtractor, MediaFormat, MediaInfo};
use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;

static SOUNDCLOUD_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:https?://)?(?:(?:www|m)\.)?soundcloud\.com/([a-zA-Z0-9_-]+/[a-zA-Z0-9_-]+)")
        .expect("Invalid SoundCloud URL regex")
});

static CLIENT_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"client_id\s*:\s*"([a-zA-Z0-9]{32})""#).expect("Invalid client_id regex")
});

pub struct SoundCloudExtractor {
    client_id_cache: RwLock<Option<String>>,
}

impl SoundCloudExtractor {
    pub fn new() -> Self {
        Self {
            client_id_cache: RwLock::new(None),
        }
    }

    pub fn matches_soundcloud_url(url: &str) -> bool {
        SOUNDCLOUD_REGEX.is_match(url)
    }

    pub async fn ensure_client_id(&self, client: &Client) -> Result<String, EngineError> {
        {
            let cache = self.client_id_cache.read().await;
            if let Some(id) = cache.as_ref() {
                return Ok(id.clone());
            }
        }

        let home_html = client
            .get("https://soundcloud.com")
            .send()
            .await?
            .text()
            .await?;
        let mut js_urls = Vec::new();
        for cap in Regex::new(r#"src="([^"]+\.js)""#)
            .unwrap()
            .captures_iter(&home_html)
        {
            if let Some(m) = cap.get(1) {
                js_urls.push(m.as_str().to_string());
            }
        }

        for js_url in js_urls.iter().rev() {
            if let Ok(resp) = client.get(js_url).send().await
                && let Ok(js_content) = resp.text().await
                && let Some(cap) = CLIENT_ID_REGEX.captures(&js_content)
                && let Some(cid) = cap.get(1)
            {
                let client_id = cid.as_str().to_string();
                let mut cache = self.client_id_cache.write().await;
                *cache = Some(client_id.clone());
                return Ok(client_id);
            }
        }

        Err(EngineError::Network(
            "Could not extract SoundCloud client_id".to_string(),
        ))
    }

    pub fn parse_track_json(
        &self,
        json: &Value,
        webpage_url: &str,
    ) -> Result<MediaInfo, EngineError> {
        let id = json["id"].as_u64().unwrap_or(0).to_string();
        if id == "0" {
            return Err(EngineError::StreamNotFound(
                "Invalid SoundCloud track object".into(),
            ));
        }

        let title = json["title"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string();
        let uploader = traverse_string(json, &["user", "username"]);
        let description = json["description"].as_str().map(String::from);
        let duration = json["duration"].as_u64().map(|d| (d / 1000) as u32);
        let thumbnail = json["artwork_url"].as_str().map(String::from);

        let mut formats = Vec::new();
        if let Some(transcodings) = json["media"]["transcodings"].as_array() {
            for tc in transcodings {
                let mime = tc["format"]["mime_type"].as_str().unwrap_or("");
                let protocol = tc["format"]["protocol"].as_str().unwrap_or("");
                let tc_url = tc["url"].as_str().unwrap_or("");

                let (ext, acodec) = if mime.contains("ogg") || mime.contains("opus") {
                    ("opus", "opus")
                } else {
                    ("mp3", "mp3")
                };

                let format_id = format!("{}_{}", protocol, ext);
                let tbr = if ext == "opus" {
                    Some(64.0)
                } else {
                    Some(128.0)
                };

                formats.push(MediaFormat {
                    format_id,
                    url: tc_url.to_string(),
                    ext: ext.to_string(),
                    width: None,
                    height: None,
                    fps: None,
                    vcodec: Some("none".into()),
                    acodec: Some(acodec.into()),
                    filesize: None,
                    tbr,
                    protocol: Some(protocol.into()),
                    http_headers: None,
                });
            }
        }

        Ok(MediaInfo {
            id,
            title,
            description,
            uploader,
            duration,
            thumbnail,
            webpage_url: Some(webpage_url.to_string()),
            formats,
            subtitles: None,
        })
    }
}

impl Default for SoundCloudExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncExtractor for SoundCloudExtractor {
    fn name(&self) -> &'static str {
        "soundcloud"
    }

    fn matches_url(&self, url: &str) -> bool {
        SOUNDCLOUD_REGEX.is_match(url)
    }

    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaInfo, EngineError> {
        let client_id = self.ensure_client_id(client).await?;
        let resolve_url = format!(
            "https://api-v2.soundcloud.com/resolve?url={}&client_id={}",
            urlencoding::encode(url),
            client_id
        );

        let json: Value = client.get(&resolve_url).send().await?.json().await?;
        self.parse_track_json(&json, url)
    }
}

#[async_trait]
impl PlatformExtractor for SoundCloudExtractor {
    async fn extract_metadata(
        &self,
        client: &Client,
        url: &str,
    ) -> Result<MediaMetadata, EngineError> {
        let client_id = self.ensure_client_id(client).await?;
        let resolve_url = format!(
            "https://api-v2.soundcloud.com/resolve?url={}&client_id={}",
            urlencoding::encode(url),
            client_id
        );

        let json: Value = client.get(&resolve_url).send().await?.json().await?;
        let id = json["id"].as_u64().unwrap_or(0).to_string();
        let title = json["title"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string();
        let artist = json["user"]["username"]
            .as_str()
            .unwrap_or("Unknown Artist")
            .to_string();
        let duration_seconds = json["duration"].as_u64().unwrap_or(0) / 1000;
        let thumbnail_url = json["artwork_url"].as_str().map(String::from);

        Ok(MediaMetadata {
            id,
            title: clean_html(&title),
            artist: clean_html(&artist),
            album: None,
            duration_seconds,
            thumbnail_url,
            webpage_url: url.to_string(),
        })
    }

    async fn search(
        &self,
        client: &Client,
        query: &str,
    ) -> Result<Vec<MediaMetadata>, EngineError> {
        let client_id = self.ensure_client_id(client).await?;
        let search_url = format!(
            "https://api-v2.soundcloud.com/search/tracks?q={}&client_id={}&limit=5",
            urlencoding::encode(query),
            client_id
        );

        let json: Value = client.get(&search_url).send().await?.json().await?;
        let mut results = Vec::new();

        if let Some(collection) = json["collection"].as_array() {
            for item in collection {
                let id = item["id"].as_u64().unwrap_or(0).to_string();
                let title = item["title"]
                    .as_str()
                    .unwrap_or("Unknown Title")
                    .to_string();
                let artist = item["user"]["username"]
                    .as_str()
                    .unwrap_or("Unknown Artist")
                    .to_string();
                let duration_seconds = item["duration"].as_u64().unwrap_or(0) / 1000;
                let thumbnail_url = item["artwork_url"].as_str().map(String::from);
                let webpage_url = item["permalink_url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();

                results.push(MediaMetadata {
                    id,
                    title: clean_html(&title),
                    artist: clean_html(&artist),
                    album: None,
                    duration_seconds,
                    thumbnail_url,
                    webpage_url,
                });
            }
        }

        Ok(results)
    }

    async fn get_stream_url(
        &self,
        client: &Client,
        track_id: &str,
    ) -> Result<StreamCandidate, EngineError> {
        let client_id = self.ensure_client_id(client).await?;
        let track_api = format!(
            "https://api-v2.soundcloud.com/tracks/{}?client_id={}",
            track_id, client_id
        );

        let json: Value = client.get(&track_api).send().await?.json().await?;
        let transcodings = json["media"]["transcodings"]
            .as_array()
            .ok_or_else(|| EngineError::StreamNotFound("No transcodings found".to_string()))?;

        // 1. Prioritize progressive MP3
        for tc in transcodings {
            let format_protocol = tc["format"]["protocol"].as_str().unwrap_or("");
            if format_protocol == "progressive" {
                let url_api = format!(
                    "{}?client_id={}",
                    tc["url"].as_str().unwrap_or_default(),
                    client_id
                );
                let stream_json: Value = client.get(&url_api).send().await?.json().await?;
                if let Some(stream_url) = stream_json["url"].as_str() {
                    return Ok(StreamCandidate {
                        format_id: "soundcloud_mp3".to_string(),
                        url: stream_url.to_string(),
                        ext: "mp3".to_string(),
                        resolution: None,
                        filesize_bytes: None,
                        tbr_kbps: Some(128.0),
                    });
                }
            }
        }

        // 2. Fall back to HLS stream
        for tc in transcodings {
            let format_protocol = tc["format"]["protocol"].as_str().unwrap_or("");
            if format_protocol == "hls" {
                let url_api = format!(
                    "{}?client_id={}",
                    tc["url"].as_str().unwrap_or_default(),
                    client_id
                );
                let stream_json: Value = client.get(&url_api).send().await?.json().await?;
                if let Some(stream_url) = stream_json["url"].as_str() {
                    return Ok(StreamCandidate {
                        format_id: "soundcloud_hls".to_string(),
                        url: stream_url.to_string(),
                        ext: "mp3".to_string(),
                        resolution: None,
                        filesize_bytes: None,
                        tbr_kbps: Some(128.0),
                    });
                }
            }
        }

        Err(EngineError::StreamNotFound(
            "No supported stream candidate found for SoundCloud track".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_soundcloud_url_matching() {
        assert!(SoundCloudExtractor::matches_soundcloud_url(
            "https://soundcloud.com/artist-name/track-name"
        ));
        assert!(SoundCloudExtractor::matches_soundcloud_url(
            "http://m.soundcloud.com/artist/title?in=playlist"
        ));
        assert!(!SoundCloudExtractor::matches_soundcloud_url(
            "https://example.com/not_soundcloud"
        ));
    }

    #[test]
    fn test_parse_track_json_mock() {
        let mock_track = json!({
            "id": 987654321,
            "title": "Summer Vibes",
            "description": "Chill summer groove",
            "duration": 185000,
            "artwork_url": "https://i1.sndcdn.com/artworks-123.jpg",
            "user": {
                "username": "ChillProducer"
            },
            "media": {
                "transcodings": [
                    {
                        "url": "https://api-v2.soundcloud.com/media/progressive",
                        "format": {
                            "protocol": "progressive",
                            "mime_type": "audio/mpeg"
                        }
                    },
                    {
                        "url": "https://api-v2.soundcloud.com/media/hls",
                        "format": {
                            "protocol": "hls",
                            "mime_type": "audio/ogg; codecs=\"opus\""
                        }
                    }
                ]
            }
        });

        let extractor = SoundCloudExtractor::new();
        let info = extractor
            .parse_track_json(&mock_track, "https://soundcloud.com/artist/track")
            .expect("Failed to parse SoundCloud mock JSON");

        assert_eq!(info.id, "987654321");
        assert_eq!(info.title, "Summer Vibes");
        assert_eq!(info.uploader.as_deref(), Some("ChillProducer"));
        assert_eq!(info.duration, Some(185));
        assert_eq!(info.formats.len(), 2);

        let best_audio = info.best_audio().expect("Missing best audio format");
        assert_eq!(best_audio.format_id, "progressive_mp3");
        assert_eq!(best_audio.acodec.as_deref(), Some("mp3"));
        assert_eq!(best_audio.is_audio_only(), true);
    }
}
