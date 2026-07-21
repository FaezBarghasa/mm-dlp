use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::sync::RwLock;

use super::PlatformExtractor;
use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;

static CLIENT_ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"client_id\s*:\s*"([a-zA-Z0-9]{32})""#).expect("Invalid client_id regex"));

pub struct SoundCloudExtractor {
    client_id_cache: RwLock<Option<String>>,
}

impl SoundCloudExtractor {
    pub fn new() -> Self {
        Self {
            client_id_cache: RwLock::new(None),
        }
    }

    async fn ensure_client_id(&self, client: &Client) -> Result<String, EngineError> {
        {
            let cache = self.client_id_cache.read().await;
            if let Some(id) = cache.as_ref() {
                return Ok(id.clone());
            }
        }

        let home_html = client.get("https://soundcloud.com").send().await?.text().await?;
        let mut js_urls = Vec::new();
        for cap in Regex::new(r#"src="([^"]+\.js)""#).unwrap().captures_iter(&home_html) {
            if let Some(m) = cap.get(1) {
                js_urls.push(m.as_str().to_string());
            }
        }

        for js_url in js_urls.iter().rev() {
            if let Ok(js_content) = client.get(js_url).send().await.and_then(|r| r.text()) {
                if let Some(cap) = CLIENT_ID_REGEX.captures(&js_content) {
                    if let Some(cid) = cap.get(1) {
                        let client_id = cid.as_str().to_string();
                        let mut cache = self.client_id_cache.write().await;
                        *cache = Some(client_id.clone());
                        return Ok(client_id);
                    }
                }
            }
        }

        Err(EngineError::Network("Could not extract SoundCloud client_id".to_string()))
    }
}

impl Default for SoundCloudExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PlatformExtractor for SoundCloudExtractor {
    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaMetadata, EngineError> {
        let client_id = self.ensure_client_id(client).await?;
        let resolve_url = format!(
            "https://api-v2.soundcloud.com/resolve?url={}&client_id={}",
            urlencoding::encode(url),
            client_id
        );

        let json: Value = client.get(&resolve_url).send().await?.json().await?;
        let id = json["id"].as_u64().unwrap_or(0).to_string();
        let title = json["title"].as_str().unwrap_or("Unknown Title").to_string();
        let artist = json["user"]["username"].as_str().unwrap_or("Unknown Artist").to_string();
        let duration_seconds = json["duration"].as_u64().unwrap_or(0) / 1000;
        let thumbnail_url = json["artwork_url"].as_str().map(String::from);

        Ok(MediaMetadata {
            id,
            title,
            artist,
            album: None,
            duration_seconds,
            thumbnail_url,
            webpage_url: url.to_string(),
        })
    }

    async fn search(&self, client: &Client, query: &str) -> Result<Vec<MediaMetadata>, EngineError> {
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
                let title = item["title"].as_str().unwrap_or("Unknown Title").to_string();
                let artist = item["user"]["username"].as_str().unwrap_or("Unknown Artist").to_string();
                let duration_seconds = item["duration"].as_u64().unwrap_or(0) / 1000;
                let thumbnail_url = item["artwork_url"].as_str().map(String::from);
                let webpage_url = item["permalink_url"].as_str().unwrap_or_default().to_string();

                results.push(MediaMetadata {
                    id,
                    title,
                    artist,
                    album: None,
                    duration_seconds,
                    thumbnail_url,
                    webpage_url,
                });
            }
        }

        Ok(results)
    }

    async fn get_stream_url(&self, client: &Client, track_id: &str) -> Result<StreamCandidate, EngineError> {
        let client_id = self.ensure_client_id(client).await?;
        let track_api = format!(
            "https://api-v2.soundcloud.com/tracks/{}?client_id={}",
            track_id, client_id
        );

        let json: Value = client.get(&track_api).send().await?.json().await?;
        let transcodings = json["media"]["transcodings"]
            .as_array()
            .ok_or_else(|| EngineError::StreamNotFound("No transcodings found".to_string()))?;

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

        Err(EngineError::StreamNotFound("No progressive MP3 stream candidate found".to_string()))
    }
}
