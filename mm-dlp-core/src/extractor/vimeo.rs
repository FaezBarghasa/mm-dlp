use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::PlatformExtractor;
use super::common::traverse_string;
use super::traits::{AsyncExtractor, MediaFormat, MediaInfo, Subtitle};
use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;

static VIMEO_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:https?://)?(?:(?:www|player)\.)?vimeo\.com/(?:video/|channels/(?:[^/?#]+/)?|)(\d+)",
    )
    .expect("Invalid Vimeo URL regex")
});

pub struct VimeoExtractor;

impl VimeoExtractor {
    pub fn new() -> Self {
        Self
    }

    pub fn extract_video_id(url: &str) -> Option<String> {
        let captures = VIMEO_REGEX.captures(url)?;
        captures.get(1).map(|m| m.as_str().to_string())
    }

    pub async fn fetch_player_config(
        &self,
        client: &Client,
        video_id: &str,
    ) -> Result<Value, EngineError> {
        let config_url = format!("https://player.vimeo.com/video/{}/config", video_id);
        let resp = client
            .get(&config_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
            .header("Referer", format!("https://vimeo.com/{}", video_id))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(EngineError::StreamNotFound(format!(
                "Vimeo config request failed with HTTP {}",
                resp.status()
            )));
        }

        let json: Value = resp.json().await?;
        Ok(json)
    }

    pub fn parse_config(&self, json: &Value, video_id: &str) -> Result<MediaInfo, EngineError> {
        let video = &json["video"];
        if !video.is_object() {
            return Err(EngineError::StreamNotFound(
                "Vimeo video metadata missing in config".into(),
            ));
        }

        let title = video["title"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string();
        let uploader = traverse_string(video, &["owner", "name"]);
        let duration = video["duration"].as_u64().map(|d| d as u32);

        // Thumbnail extraction: pick largest resolution thumb from video.thumbs
        let thumbnail = if let Some(thumbs) = video["thumbs"].as_object() {
            thumbs
                .iter()
                .max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0))
                .and_then(|(_, v)| v.as_str())
                .map(String::from)
        } else {
            video["thumbnail_url"].as_str().map(String::from)
        };

        let mut formats = Vec::new();
        let request_files = &json["request"]["files"];

        // 1. Progressive HTTP MP4 formats
        if let Some(progressive) = request_files["progressive"].as_array() {
            for item in progressive {
                if let Some(url) = item["url"].as_str() {
                    let quality = item["quality"].as_str().unwrap_or("unknown");
                    let width = item["width"].as_u64().map(|w| w as u32);
                    let height = item["height"].as_u64().map(|h| h as u32);
                    let fps = item["fps"].as_f64();
                    let tbr = item["bitrate"].as_f64().map(|b| b / 1000.0);

                    formats.push(MediaFormat {
                        format_id: format!("http-{}", quality),
                        url: url.to_string(),
                        ext: "mp4".into(),
                        width,
                        height,
                        fps,
                        vcodec: Some("h264".into()),
                        acodec: Some("aac".into()),
                        filesize: None,
                        tbr,
                        protocol: Some("https".into()),
                        http_headers: None,
                    });
                }
            }
        }

        // 2. HLS formats (m3u8 CDN entries)
        if let Some(hls_cdns) = request_files["hls"]["cdns"].as_object() {
            for (cdn_name, cdn_data) in hls_cdns {
                if let Some(url) = cdn_data["url"].as_str() {
                    formats.push(MediaFormat {
                        format_id: format!("hls-{}", cdn_name),
                        url: url.to_string(),
                        ext: "mp4".into(),
                        width: None,
                        height: None,
                        fps: None,
                        vcodec: Some("h264".into()),
                        acodec: Some("aac".into()),
                        filesize: None,
                        tbr: None,
                        protocol: Some("m3u8".into()),
                        http_headers: None,
                    });
                }
            }
        }

        // Subtitles extraction from request.text_tracks
        let mut subtitles: HashMap<String, Vec<Subtitle>> = HashMap::new();
        if let Some(tracks) = json["request"]["text_tracks"].as_array() {
            for track in tracks {
                if let (Some(lang), Some(url)) = (track["lang"].as_str(), track["url"].as_str()) {
                    let full_url = if url.starts_with("http") {
                        url.to_string()
                    } else {
                        format!("https://player.vimeo.com{}", url)
                    };

                    subtitles
                        .entry(lang.to_string())
                        .or_default()
                        .push(Subtitle {
                            ext: "vtt".into(),
                            url: Some(full_url),
                            data: None,
                        });
                }
            }
        }

        Ok(MediaInfo {
            id: video_id.to_string(),
            title,
            description: None,
            uploader,
            duration,
            thumbnail,
            webpage_url: Some(format!("https://vimeo.com/{}", video_id)),
            formats,
            subtitles: if subtitles.is_empty() {
                None
            } else {
                Some(subtitles)
            },
        })
    }
}

impl Default for VimeoExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncExtractor for VimeoExtractor {
    fn name(&self) -> &'static str {
        "vimeo"
    }

    fn matches_url(&self, url: &str) -> bool {
        VIMEO_REGEX.is_match(url)
    }

    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaInfo, EngineError> {
        let video_id = Self::extract_video_id(url)
            .ok_or_else(|| EngineError::UnsupportedUrl(format!("Invalid Vimeo URL: {}", url)))?;

        let json = self.fetch_player_config(client, &video_id).await?;
        self.parse_config(&json, &video_id)
    }
}

#[async_trait]
impl PlatformExtractor for VimeoExtractor {
    async fn extract_metadata(
        &self,
        client: &Client,
        url: &str,
    ) -> Result<MediaMetadata, EngineError> {
        let video_id = Self::extract_video_id(url)
            .ok_or_else(|| EngineError::UnsupportedUrl(format!("Invalid Vimeo URL: {}", url)))?;

        let json = self.fetch_player_config(client, &video_id).await?;
        let video = &json["video"];

        let title = video["title"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string();
        let artist = traverse_string(video, &["owner", "name"]).unwrap_or_else(|| "Unknown".into());
        let duration_seconds = video["duration"].as_u64().unwrap_or(0);
        let thumbnail_url = video["thumbs"]
            .as_object()
            .and_then(|thumbs| thumbs.values().next_back())
            .and_then(|t| t.as_str())
            .map(String::from);

        Ok(MediaMetadata {
            id: video_id.clone(),
            title,
            artist,
            album: None,
            duration_seconds,
            thumbnail_url,
            webpage_url: format!("https://vimeo.com/{}", video_id),
        })
    }

    async fn search(
        &self,
        _client: &Client,
        _query: &str,
    ) -> Result<Vec<MediaMetadata>, EngineError> {
        // Vimeo public search requires OAuth API app authorization
        Err(EngineError::UnsupportedUrl(
            "Vimeo direct public search requires API keys".into(),
        ))
    }

    async fn get_stream_url(
        &self,
        client: &Client,
        track_id: &str,
    ) -> Result<StreamCandidate, EngineError> {
        let json = self.fetch_player_config(client, track_id).await?;
        let info = self.parse_config(&json, track_id)?;

        let best_format = info.best_format().ok_or_else(|| {
            EngineError::StreamNotFound("No stream formats found for Vimeo video".into())
        })?;

        Ok(StreamCandidate {
            format_id: best_format.format_id.clone(),
            url: best_format.url.clone(),
            ext: best_format.ext.clone(),
            resolution: best_format.height.map(|h| format!("{}p", h)),
            filesize_bytes: best_format.filesize,
            tbr_kbps: best_format.tbr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_vimeo_id() {
        assert_eq!(
            VimeoExtractor::extract_video_id("https://vimeo.com/123456789"),
            Some("123456789".into())
        );
        assert_eq!(
            VimeoExtractor::extract_video_id("https://player.vimeo.com/video/987654321?h=abc"),
            Some("987654321".into())
        );
        assert_eq!(
            VimeoExtractor::extract_video_id("https://vimeo.com/channels/staffpicks/54469442"),
            Some("54469442".into())
        );
        assert_eq!(
            VimeoExtractor::extract_video_id("https://example.com/not_vimeo"),
            None
        );
    }

    #[test]
    fn test_parse_vimeo_config_mock() {
        let mock_config = json!({
            "video": {
                "id": 123456789,
                "title": "Animation Reel",
                "duration": 95,
                "owner": {
                    "name": "Jane Doe",
                    "url": "https://vimeo.com/janedoe"
                },
                "thumbs": {
                    "640": "https://i.vimeocdn.com/video/123_640.jpg",
                    "1280": "https://i.vimeocdn.com/video/123_1280.jpg"
                }
            },
            "request": {
                "files": {
                    "progressive": [
                        {
                            "profile": 113,
                            "width": 1280,
                            "height": 720,
                            "mime": "video/mp4",
                            "fps": 30,
                            "url": "https://vod-progressive.akamaized.net/720p.mp4",
                            "quality": "720p",
                            "bitrate": 2500000
                        },
                        {
                            "profile": 119,
                            "width": 1920,
                            "height": 1080,
                            "mime": "video/mp4",
                            "fps": 60,
                            "url": "https://vod-progressive.akamaized.net/1080p.mp4",
                            "quality": "1080p",
                            "bitrate": 5000000
                        }
                    ],
                    "hls": {
                        "cdns": {
                            "akfire_interconnect_quic": {
                                "url": "https://vod-adaptive.akamaized.net/master.m3u8"
                            }
                        }
                    }
                },
                "text_tracks": [
                    {
                        "id": 1,
                        "lang": "en",
                        "url": "/texttrack/123.vtt",
                        "label": "English"
                    }
                ]
            }
        });

        let extractor = VimeoExtractor::new();
        let info = extractor
            .parse_config(&mock_config, "123456789")
            .expect("Failed to parse Vimeo mock config");

        assert_eq!(info.id, "123456789");
        assert_eq!(info.title, "Animation Reel");
        assert_eq!(info.uploader.as_deref(), Some("Jane Doe"));
        assert_eq!(info.duration, Some(95));
        assert_eq!(
            info.thumbnail.as_deref(),
            Some("https://i.vimeocdn.com/video/123_1280.jpg")
        );
        assert_eq!(info.formats.len(), 3);

        let best_video = info.best_video().expect("No video format found");
        assert_eq!(best_video.format_id, "http-1080p");
        assert_eq!(best_video.height, Some(1080));
        assert_eq!(best_video.fps, Some(60.0));

        let subs = info.subtitles.expect("Missing subtitles");
        assert!(subs.contains_key("en"));
        assert_eq!(
            subs["en"][0].url.as_deref(),
            Some("https://player.vimeo.com/texttrack/123.vtt")
        );
    }
}
