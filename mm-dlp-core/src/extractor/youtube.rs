use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::PlatformExtractor;
use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;

pub struct YouTubeExtractor;

impl YouTubeExtractor {
    pub fn new() -> Self {
        Self
    }

    fn extract_video_id(url: &str) -> Option<String> {
        if let Some(pos) = url.find("v=") {
            let rest = &url[pos + 2..];
            let id = rest.split('&').next().unwrap_or(rest);
            return Some(id.to_string());
        }
        if let Some(pos) = url.find("youtu.be/") {
            let rest = &url[pos + 9..];
            let id = rest.split('?').next().unwrap_or(rest);
            return Some(id.to_string());
        }
        None
    }
}

impl Default for YouTubeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PlatformExtractor for YouTubeExtractor {
    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaMetadata, EngineError> {
        let video_id = Self::extract_video_id(url)
            .ok_or_else(|| EngineError::UnsupportedUrl(format!("Invalid YouTube URL: {}", url)))?;

        let body = serde_json::json!({
            "videoId": video_id,
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20231010.00.00"
                }
            }
        });

        let resp = client
            .post("https://www.youtube.com/youtubei/v1/player")
            .json(&body)
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let video_details = &json["videoDetails"];

        let title = video_details["title"].as_str().unwrap_or("Unknown Title").to_string();
        let artist = video_details["author"].as_str().unwrap_or("Unknown Artist").to_string();
        let duration_seconds = video_details["lengthSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let thumbnail_url = video_details["thumbnail"]["thumbnails"]
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|t| t["url"].as_str())
            .map(String::from);

        Ok(MediaMetadata {
            id: video_id,
            title,
            artist,
            album: None,
            duration_seconds,
            thumbnail_url,
            webpage_url: format!("https://www.youtube.com/watch?v={}", url),
        })
    }

    async fn search(&self, client: &Client, query: &str) -> Result<Vec<MediaMetadata>, EngineError> {
        let body = serde_json::json!({
            "query": query,
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20231010.00.00"
                }
            }
        });

        let resp = client
            .post("https://www.youtube.com/youtubei/v1/search")
            .json(&body)
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let mut results = Vec::new();

        if let Some(contents) = json["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]["sectionListRenderer"]["contents"].as_array() {
            for section in contents {
                if let Some(items) = section["itemSectionRenderer"]["contents"].as_array() {
                    for item in items {
                        let render = &item["videoRenderer"];
                        if render.is_object() {
                            let video_id = render["videoId"].as_str().unwrap_or_default().to_string();
                            let title = render["title"]["runs"][0]["text"].as_str().unwrap_or("Unknown Title").to_string();
                            let artist = render["ownerText"]["runs"][0]["text"].as_str().unwrap_or("Unknown Artist").to_string();
                            let webpage_url = format!("https://www.youtube.com/watch?v={}", video_id);

                            results.push(MediaMetadata {
                                id: video_id,
                                title,
                                artist,
                                album: None,
                                duration_seconds: 0,
                                thumbnail_url: None,
                                webpage_url,
                            });
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    async fn get_stream_url(&self, client: &Client, track_id: &str) -> Result<StreamCandidate, EngineError> {
        let body = serde_json::json!({
            "videoId": track_id,
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20231010.00.00"
                }
            }
        });

        let resp = client
            .post("https://www.youtube.com/youtubei/v1/player")
            .json(&body)
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let adaptive_formats = json["streamingData"]["adaptiveFormats"]
            .as_array()
            .ok_or_else(|| EngineError::StreamNotFound("No adaptive formats found".to_string()))?;

        let mut audio_formats: Vec<&Value> = adaptive_formats
            .iter()
            .filter(|f| f["mimeType"].as_str().unwrap_or("").starts_with("audio/"))
            .collect();

        audio_formats.sort_by_key(|f| f["bitrate"].as_u64().unwrap_or(0));
        let best_format = audio_formats
            .last()
            .ok_or_else(|| EngineError::StreamNotFound("No audio formats available".to_string()))?;

        let url = best_format["url"]
            .as_str()
            .ok_or_else(|| EngineError::StreamNotFound("Audio stream URL missing in YouTube payload".to_string()))?
            .to_string();

        let ext = if best_format["mimeType"].as_str().unwrap_or("").contains("webm") {
            "webm".to_string()
        } else {
            "m4a".to_string()
        };

        let bitrate = best_format["bitrate"].as_f64().map(|b| b / 1000.0);
        let filesize = best_format["contentLength"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok());

        Ok(StreamCandidate {
            format_id: best_format["itag"].as_u64().unwrap_or(0).to_string(),
            url,
            ext,
            resolution: None,
            filesize_bytes: filesize,
            tbr_kbps: bitrate,
        })
    }
}
