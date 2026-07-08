use async_trait::async_trait;
use anyhow::{anyhow, Result};
use crate::extractor::traits::{AudioPlatformExtractor, AudioQuality, StreamInfo, TrackMetadata, AudioSource};
use serde_json::Value;
use std::time::Duration;
use regex::Regex;

const MAX_RETRIES: u32 = 3;

pub struct SoundCloudExtractor {
    client: reqwest::Client,
    client_id: String,
}

impl SoundCloudExtractor {
    pub async fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()?;
        let client_id = Self::fetch_client_id(&client).await?;
        Ok(Self { client, client_id })
    }

    /// Retries an async operation up to `MAX_RETRIES` times with exponential backoff.
    async fn with_retry<F, Fut, T>(mut f: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..MAX_RETRIES {
            match f().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < MAX_RETRIES - 1 {
                        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("Operation failed after {} retries", MAX_RETRIES)))
    }

    /// Dynamically fetches the SoundCloud `client_id` from their frontend JS bundles.
    async fn fetch_client_id(client: &reqwest::Client) -> Result<String> {
        let homepage = Self::with_retry(|| async {
            client
                .get("https://soundcloud.com")
                .send()
                .await
                .map_err(|e| anyhow!("SC homepage request failed: {}", e))?
                .text()
                .await
                .map_err(|e| anyhow!("SC homepage body read failed: {}", e))
        })
        .await?;

        let script_regex = Regex::new(
            r#"<script crossorigin src="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)"></script>"#,
        )
        .map_err(|e| anyhow!("Script regex compile failed: {}", e))?;

        let client_id_regex = Regex::new(r#",client_id:"([a-zA-Z0-9]+)""#)
            .map_err(|e| anyhow!("client_id regex compile failed: {}", e))?;

        for cap in script_regex.captures_iter(&homepage) {
            let script_url = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let script_code = match Self::with_retry(|| async {
                client
                    .get(script_url)
                    .send()
                    .await
                    .map_err(|e| anyhow!("SC script fetch failed: {}", e))?
                    .text()
                    .await
                    .map_err(|e| anyhow!("SC script body read failed: {}", e))
            })
            .await
            {
                Ok(code) => code,
                Err(_) => continue, // Skip this script if it fails; try the next one
            };

            if let Some(client_id_cap) = client_id_regex.captures(&script_code) {
                let id = client_id_cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .ok_or_else(|| anyhow!("client_id capture group empty"))?;
                return Ok(id);
            }
        }

        Err(anyhow!("Failed to extract SoundCloud client_id from any JS bundle"))
    }
}

#[async_trait]
impl AudioPlatformExtractor for SoundCloudExtractor {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>> {
        let search_url = format!(
            "https://api-v2.soundcloud.com/search/tracks?q={}&client_id={}",
            urlencoding::encode(query),
            self.client_id
        );

        let response: Value = Self::with_retry(|| async {
            self.client
                .get(&search_url)
                .send()
                .await
                .map_err(|e| anyhow!("SC search request failed: {}", e))?
                .json()
                .await
                .map_err(|e| anyhow!("SC search response parse failed: {}", e))
        })
        .await?;

        let mut tracks = Vec::new();
        let collection = response["collection"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        for track_data in &collection {
            let track_id = match track_data["id"].as_u64() {
                Some(id) => id.to_string(),
                None => continue,
            };

            let title = track_data["title"]
                .as_str()
                .unwrap_or("Unknown Title")
                .to_string();
            let artist = track_data["user"]["username"]
                .as_str()
                .unwrap_or("Unknown Artist")
                .to_string();
            let album_art_url = track_data["artwork_url"]
                .as_str()
                .map(str::to_string);

            tracks.push(TrackMetadata {
                title,
                artist,
                album: None,
                album_art_url,
                track_id,
                source: AudioSource::SoundCloud,
            });
        }

        Ok(tracks)
    }

    async fn get_stream_url(&self, track_id: &str, _quality: AudioQuality) -> Result<StreamInfo> {
        let track_url = format!(
            "https://api-v2.soundcloud.com/tracks/{}?client_id={}",
            track_id, self.client_id
        );

        let track_data: Value = Self::with_retry(|| async {
            self.client
                .get(&track_url)
                .send()
                .await
                .map_err(|e| anyhow!("SC track request failed: {}", e))?
                .json()
                .await
                .map_err(|e| anyhow!("SC track response parse failed: {}", e))
        })
        .await?;

        // Prefer progressive (direct MP3), fall back to HLS
        let transcodings = track_data["media"]["transcodings"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let media_url = transcodings
            .iter()
            .find(|f| f["format"]["protocol"].as_str() == Some("progressive"))
            .or_else(|| {
                transcodings
                    .iter()
                    .find(|f| f["format"]["protocol"].as_str() == Some("hls"))
            })
            .and_then(|f| f["url"].as_str())
            .ok_or_else(|| anyhow!("No progressive or HLS stream found for track {}", track_id))?
            .to_string();

        let stream_response: Value = Self::with_retry(|| async {
            self.client
                .get(&media_url)
                .query(&[("client_id", &self.client_id)])
                .send()
                .await
                .map_err(|e| anyhow!("SC stream URL request failed: {}", e))?
                .json()
                .await
                .map_err(|e| anyhow!("SC stream URL parse failed: {}", e))
        })
        .await?;

        let stream_url = stream_response["url"]
            .as_str()
            .ok_or_else(|| anyhow!("No 'url' field in SC stream response"))?
            .to_string();

        let metadata = TrackMetadata {
            title: track_data["title"]
                .as_str()
                .unwrap_or("Unknown Title")
                .to_string(),
            artist: track_data["user"]["username"]
                .as_str()
                .unwrap_or("Unknown Artist")
                .to_string(),
            album: None,
            album_art_url: track_data["artwork_url"].as_str().map(str::to_string),
            track_id: track_id.to_string(),
            source: AudioSource::SoundCloud,
        };

        // SoundCloud progressive streams are typically 128kbps MP3
        let bitrate = track_data["bitrate"]
            .as_u64()
            .unwrap_or(128) as u32;
        let duration_secs = track_data["duration"].as_u64().unwrap_or(0) / 1000;

        Ok(StreamInfo {
            stream_url,
            format: "audio/mpeg".to_string(),
            bitrate,
            duration_secs,
            metadata,
        })
    }
}
