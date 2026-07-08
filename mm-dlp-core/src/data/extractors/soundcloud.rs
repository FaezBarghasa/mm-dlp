use async_trait::async_trait;
use anyhow::{anyhow, Result};
use crate::extractor::traits::{AudioPlatformExtractor, AudioQuality, StreamInfo, TrackMetadata, AudioSource};
use serde_json::Value;
use std::time::Duration;
use regex::Regex;

pub struct SoundCloudExtractor {
    client: reqwest::Client,
    client_id: String,
}

impl SoundCloudExtractor {
    pub async fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        let client_id = Self::fetch_client_id(&client).await?;
        Ok(Self { client, client_id })
    }

    async fn fetch_client_id(client: &reqwest::Client) -> Result<String> {
        let response = client.get("https://soundcloud.com").send().await?.text().await?;
        let script_regex = Regex::new(r#"<script crossorigin src="(https://a-v2.sndcdn.com/assets/.*?.js)"></script>"#)?;

        for cap in script_regex.captures_iter(&response) {
            let script_url = &cap[1];
            let script_code = client.get(script_url).send().await?.text().await?;
            let client_id_regex = Regex::new(r#",client_id:"([a-zA-Z0-9]+)""#)?;
            if let Some(client_id_cap) = client_id_regex.captures(&script_code) {
                return Ok(client_id_cap[1].to_string());
            }
        }
        Err(anyhow!("Failed to fetch SoundCloud client_id"))
    }
}

#[async_trait]
impl AudioPlatformExtractor for SoundCloudExtractor {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>> {
        let search_url = format!("https://api-v2.soundcloud.com/search/tracks?q={}&client_id={}", query, self.client_id);
        let response: Value = self.client.get(&search_url).send().await?.json().await?;

        let mut tracks = Vec::new();
        if let Some(collection) = response["collection"].as_array() {
            for track_data in collection {
                tracks.push(TrackMetadata {
                    title: track_data["title"].as_str().unwrap_or("").to_string(),
                    artist: track_data["user"]["username"].as_str().unwrap_or("").to_string(),
                    album: None,
                    album_art_url: track_data["artwork_url"].as_str().map(|s| s.to_string()),
                    track_id: track_data["id"].as_u64().unwrap_or(0).to_string(),
                    source: AudioSource::SoundCloud,
                });
            }
        }
        Ok(tracks)
    }

    async fn get_stream_url(&self, track_id: &str, _quality: AudioQuality) -> Result<StreamInfo> {
        let track_url = format!("https://api-v2.soundcloud.com/tracks/{}?client_id={}", track_id, self.client_id);
        let track_data: Value = self.client.get(&track_url).send().await?.json().await?;

        let media_url = track_data["media"]["transcodings"]
            .as_array()
            .and_then(|t| t.iter().find(|f| f["format"]["protocol"] == "progressive"))
            .and_then(|f| f["url"].as_str())
            .ok_or_else(|| anyhow!("No progressive stream found"))?;

        let stream_url_response: Value = self.client.get(media_url).query(&[("client_id", &self.client_id)]).send().await?.json().await?;
        let stream_url = stream_url_response["url"].as_str().ok_or_else(|| anyhow!("No stream URL in response"))?.to_string();

        let metadata = TrackMetadata {
            title: track_data["title"].as_str().unwrap_or("").to_string(),
            artist: track_data["user"]["username"].as_str().unwrap_or("").to_string(),
            album: None,
            album_art_url: track_data["artwork_url"].as_str().map(|s| s.to_string()),
            track_id: track_id.to_string(),
            source: AudioSource::SoundCloud,
        };

        Ok(StreamInfo {
            stream_url,
            format: "mp3".to_string(),
            bitrate: 128, // SoundCloud progressive is typically 128kbps mp3
            duration_secs: track_data["duration"].as_u64().unwrap_or(0) / 1000,
            metadata,
        })
    }
}
