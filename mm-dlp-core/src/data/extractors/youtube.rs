use async_trait::async_trait;
use anyhow::{anyhow, Result};
use crate::extractor::traits::{AudioPlatformExtractor, AudioQuality, StreamInfo, TrackMetadata, AudioSource};
use crate::js::decipher::JsDecipher;
use serde_json::Value;
use std::time::Duration;

/// Number of retries for network calls.
const MAX_RETRIES: u32 = 3;

pub struct YouTubeMusicExtractor {
    client: reqwest::Client,
    decipher: JsDecipher,
}

impl YouTubeMusicExtractor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
                .build()?,
            decipher: JsDecipher::new()?,
        })
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

    /// Fetches audio stream info for a given video ID using the YouTube player page.
    async fn fetch_stream_info(&self, track_id: &str) -> Result<StreamInfo> {
        let video_url = format!("https://www.youtube.com/watch?v={}", track_id);
        let response_text = Self::with_retry(|| async {
            self.client
                .get(&video_url)
                .send()
                .await
                .map_err(|e| anyhow!("Request failed: {}", e))?
                .text()
                .await
                .map_err(|e| anyhow!("Body read failed: {}", e))
        })
        .await?;

        // Extract ytInitialPlayerResponse JSON blob
        let player_response_regex =
            regex::Regex::new(r"var ytInitialPlayerResponse\s*=\s*(\{.+?\});")
                .map_err(|e| anyhow!("Regex compile error: {}", e))?;
        let player_response_str = player_response_regex
            .captures(&response_text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .ok_or_else(|| anyhow!("ytInitialPlayerResponse not found in page"))?;

        let player_response: Value = serde_json::from_str(player_response_str)
            .map_err(|e| anyhow!("Failed to parse player response JSON: {}", e))?;

        let adaptive_formats = player_response["streamingData"]["adaptiveFormats"]
            .as_array()
            .ok_or_else(|| anyhow!("No adaptiveFormats in streamingData"))?;

        // Prefer Opus (mime starts with "audio/webm"), then fall back to AAC (audio/mp4)
        let audio_format = adaptive_formats
            .iter()
            .filter(|f| {
                f["mimeType"]
                    .as_str()
                    .map(|m| m.starts_with("audio/"))
                    .unwrap_or(false)
            })
            .max_by(|a, b| {
                let a_opus = a["mimeType"].as_str().unwrap_or("").contains("opus");
                let b_opus = b["mimeType"].as_str().unwrap_or("").contains("opus");
                // Prefer Opus; then by bitrate
                match (a_opus, b_opus) {
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => a["bitrate"]
                        .as_u64()
                        .unwrap_or(0)
                        .cmp(&b["bitrate"].as_u64().unwrap_or(0)),
                }
            })
            .ok_or_else(|| anyhow!("No audio format found in adaptiveFormats"))?;

        // Resolve stream URL — may be direct or cipher-protected
        let stream_url = if let Some(direct_url) = audio_format["url"].as_str() {
            direct_url.to_string()
        } else {
            let signature_cipher = audio_format["signatureCipher"]
                .as_str()
                .ok_or_else(|| anyhow!("Neither url nor signatureCipher found"))?;

            let mut base_url = String::new();
            let mut sp = String::new();
            let mut s = String::new();

            for param in signature_cipher.split('&') {
                let mut parts = param.splitn(2, '=');
                let key = parts.next().unwrap_or("");
                let value = urlencoding::decode(parts.next().unwrap_or(""))
                    .map(|s| s.into_owned())
                    .unwrap_or_default();
                match key {
                    "url" => base_url = value,
                    "sp" => sp = value,
                    "s" => s = value,
                    _ => {}
                }
            }

            if base_url.is_empty() || s.is_empty() {
                return Err(anyhow!("Could not parse signatureCipher params"));
            }

            // Fetch the player JS to extract decipher function
            let js_url_regex =
                regex::Regex::new(r#""jsUrl"\s*:\s*"(/s/player/[^"]+)""#)
                    .map_err(|e| anyhow!("jsUrl regex error: {}", e))?;
            let js_player_path = js_url_regex
                .captures(&response_text)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .ok_or_else(|| anyhow!("JS player URL not found"))?;
            let js_player_url = format!("https://www.youtube.com{}", js_player_path);

            let js_code = Self::with_retry(|| async {
                self.client
                    .get(&js_player_url)
                    .send()
                    .await
                    .map_err(|e| anyhow!("JS fetch failed: {}", e))?
                    .text()
                    .await
                    .map_err(|e| anyhow!("JS read failed: {}", e))
            })
            .await?;

            let deciphered = self
                .decipher
                .decipher(&js_code, &s)
                .map_err(|e| anyhow!("Decipher failed: {}", e))?;

            format!("{}&{}={}", base_url, sp, deciphered)
        };

        // Extract thumbnail URL safely
        let album_art_url = player_response["videoDetails"]["thumbnail"]["thumbnails"]
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|t| t["url"].as_str())
            .map(str::to_string);

        let metadata = TrackMetadata {
            title: player_response["videoDetails"]["title"]
                .as_str()
                .unwrap_or("Unknown Title")
                .to_string(),
            artist: player_response["videoDetails"]["author"]
                .as_str()
                .unwrap_or("Unknown Artist")
                .to_string(),
            album: None,
            album_art_url,
            track_id: track_id.to_string(),
            source: AudioSource::YouTubeMusic,
        };

        let format_str = audio_format["mimeType"]
            .as_str()
            .unwrap_or("audio/mp4")
            .split(';')
            .next()
            .unwrap_or("audio/mp4")
            .to_string();

        let bitrate = audio_format["bitrate"].as_u64().unwrap_or(0) as u32;

        let duration_secs = player_response["videoDetails"]["lengthSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(StreamInfo {
            stream_url,
            format: format_str,
            bitrate,
            duration_secs,
            metadata,
        })
    }
}

#[async_trait]
impl AudioPlatformExtractor for YouTubeMusicExtractor {
    async fn search(&self, query: &str) -> Result<Vec<TrackMetadata>> {
        // Use the YouTube Music InnerTube API for structured search results
        let innertube_body = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240501.01.00"
                }
            },
            "query": query
        });

        let response: Value = Self::with_retry(|| async {
            self.client
                .post("https://music.youtube.com/youtubei/v1/search?alt=json")
                .header("Content-Type", "application/json")
                .header("X-YouTube-Client-Name", "67")
                .header("X-YouTube-Client-Version", "1.20240501.01.00")
                .json(&innertube_body)
                .send()
                .await
                .map_err(|e| anyhow!("InnerTube search request failed: {}", e))?
                .json()
                .await
                .map_err(|e| anyhow!("InnerTube response parse failed: {}", e))
        })
        .await?;

        let mut tracks = Vec::new();

        // Navigate InnerTube response structure
        let tabs = response["contents"]["tabbedSearchResultsRenderer"]["tabs"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        for tab in &tabs {
            let contents = tab["tabRenderer"]["content"]["sectionListRenderer"]["contents"]
                .as_array()
                .cloned()
                .unwrap_or_default();

            for section in &contents {
                let items = section["musicShelfRenderer"]["contents"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();

                for item in &items {
                    let renderer = &item["musicResponsiveListItemRenderer"];

                    let track_id = renderer["playlistItemData"]["videoId"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    if track_id.is_empty() {
                        continue;
                    }

                    let title = renderer["flexColumns"][0]
                        ["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
                        .as_str()
                        .unwrap_or("Unknown Title")
                        .to_string();

                    let artist = renderer["flexColumns"][1]
                        ["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"][0]["text"]
                        .as_str()
                        .unwrap_or("Unknown Artist")
                        .to_string();

                    let album_art_url = renderer["thumbnail"]["musicThumbnailRenderer"]
                        ["thumbnail"]["thumbnails"]
                        .as_array()
                        .and_then(|arr| arr.last())
                        .and_then(|t| t["url"].as_str())
                        .map(str::to_string);

                    tracks.push(TrackMetadata {
                        title,
                        artist,
                        album: None,
                        album_art_url,
                        track_id,
                        source: AudioSource::YouTubeMusic,
                    });
                }
            }
        }

        Ok(tracks)
    }

    async fn get_stream_url(&self, track_id: &str, _quality: AudioQuality) -> Result<StreamInfo> {
        Self::with_retry(|| self.fetch_stream_info(track_id)).await
    }
}
