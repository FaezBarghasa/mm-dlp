use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

use super::PlatformExtractor;
use super::common::{clean_html, determine_ext, traverse_obj, traverse_string};
use super::traits::{AsyncExtractor, MediaFormat, MediaInfo, Subtitle};
use crate::config::{MediaMetadata, StreamCandidate};
use crate::error::EngineError;
use crate::js::decipher::JsDecipher;

static PLAYER_JS_CACHE: LazyLock<Arc<RwLock<Option<String>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

static JS_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:/s/player/[a-zA-Z0-9_-]+/player_ias\.vflset/[a-zA-Z0-9_-]+/base\.js|/s/player/[^"]+/base\.js)"#)
        .expect("Invalid base.js URL regex")
});

static YOUTUBE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:https?://)?(?:(?:www|m)\.)?(?:youtube\.com/(?:watch\?.*?v=|embed/|v/|shorts/|live/)|youtu\.be/)([a-zA-Z0-9_-]{11})")
        .expect("Invalid YouTube URL regex")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnertubeClientType {
    Web,
    Android,
    WebRemix,
}

pub struct YouTubeExtractor {
    client_type: InnertubeClientType,
}

impl YouTubeExtractor {
    pub fn new() -> Self {
        Self {
            client_type: InnertubeClientType::Web,
        }
    }

    pub fn with_client(client_type: InnertubeClientType) -> Self {
        Self { client_type }
    }

    pub fn extract_video_id(url: &str) -> Option<String> {
        let captures = YOUTUBE_REGEX.captures(url)?;
        captures.get(1).map(|m| m.as_str().to_string())
    }

    fn build_innertube_context(&self) -> Value {
        match self.client_type {
            InnertubeClientType::Web => serde_json::json!({
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20240708.00.00",
                    "hl": "en",
                    "gl": "US"
                }
            }),
            InnertubeClientType::Android => serde_json::json!({
                "client": {
                    "clientName": "ANDROID",
                    "clientVersion": "19.29.35",
                    "androidSdkVersion": 30,
                    "userAgent": "com.google.android.youtube/19.29.35 (Linux; U; Android 11) gzip",
                    "hl": "en",
                    "gl": "US"
                }
            }),
            InnertubeClientType::WebRemix => serde_json::json!({
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240707.12.00",
                    "hl": "en",
                    "gl": "US"
                }
            }),
        }
    }

    pub async fn fetch_player_response(
        &self,
        client: &Client,
        video_id: &str,
    ) -> Result<Value, EngineError> {
        let body = serde_json::json!({
            "videoId": video_id,
            "context": self.build_innertube_context(),
            "playbackContext": {
                "contentPlaybackContext": {
                    "html5Preference": "HTML5_PREF_WANTS"
                }
            }
        });

        let resp = client
            .post("https://www.youtube.com/youtubei/v1/player")
            .header("Content-Type", "application/json")
            .header("X-YouTube-Client-Name", "1")
            .header("X-YouTube-Client-Version", "2.20240708.00.00")
            .json(&body)
            .send()
            .await?;

        let json: Value = resp.json().await?;
        Ok(json)
    }

    pub fn parse_formats(
        &self,
        streaming_data: &Value,
        js_decipher: Option<(&JsDecipher, &str)>,
    ) -> Vec<MediaFormat> {
        let mut formats = Vec::new();
        let mut raw_formats = Vec::new();

        if let Some(muxed) = streaming_data["formats"].as_array() {
            raw_formats.extend(muxed.iter());
        }
        if let Some(adaptive) = streaming_data["adaptiveFormats"].as_array() {
            raw_formats.extend(adaptive.iter());
        }

        for item in raw_formats {
            let format_id = item["itag"]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".into());

            let mime_type = item["mimeType"].as_str().unwrap_or_default();
            let ext = if mime_type.contains("webm") {
                "webm".into()
            } else if mime_type.contains("mp4") || mime_type.contains("m4a") {
                if mime_type.starts_with("audio/") {
                    "m4a".into()
                } else {
                    "mp4".into()
                }
            } else {
                determine_ext(item["url"].as_str().unwrap_or(""))
            };

            let mut stream_url = item["url"].as_str().map(String::from);

            // Handle signatureCipher / cipher if direct url is absent
            if stream_url.is_none() {
                let cipher_str = item["signatureCipher"]
                    .as_str()
                    .or_else(|| item["cipher"].as_str());

                if let Some(cipher) = cipher_str {
                    let mut base_url = String::new();
                    let mut s = String::new();
                    let mut sp = "sig".to_string();
                    let mut n_param = String::new();

                    for pair in cipher.split('&') {
                        let mut kv = pair.splitn(2, '=');
                        let k = kv.next().unwrap_or("");
                        let v = urlencoding::decode(kv.next().unwrap_or(""))
                            .map(|cow| cow.into_owned())
                            .unwrap_or_default();
                        match k {
                            "url" => base_url = v,
                            "sp" => sp = v,
                            "s" => s = v,
                            "n" => n_param = v,
                            _ => {}
                        }
                    }

                    if !base_url.is_empty() {
                        if !n_param.is_empty() && !base_url.contains("n=") {
                            let delimiter = if base_url.contains('?') { '&' } else { '?' };
                            base_url = format!("{}{}{}={}", base_url, delimiter, "n", n_param);
                        }
                        if !s.is_empty() {
                            if let Some((decipherer, js_code)) = js_decipher
                                && let Ok(deciphered) = decipherer.decipher(js_code, &s)
                            {
                                let delimiter = if base_url.contains('?') { '&' } else { '?' };
                                stream_url =
                                    Some(format!("{}{}{}={}", base_url, delimiter, sp, deciphered));
                            }
                        } else {
                            stream_url = Some(base_url);
                        }
                    }
                }
            }

            let Some(raw_stream_url) = stream_url else {
                continue;
            };

            let final_url = if let Some((decipherer, js_code)) = js_decipher {
                Self::transform_n_parameter(decipherer, js_code, &raw_stream_url)
            } else {
                raw_stream_url
            };

            let width = item["width"].as_u64().map(|w| w as u32);
            let height = item["height"].as_u64().map(|h| h as u32);
            let fps = item["fps"].as_f64();
            let filesize = item["contentLength"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok());
            let tbr = item["bitrate"].as_f64().map(|b| b / 1000.0);

            let vcodec = if mime_type.starts_with("video/") {
                item["mimeType"]
                    .as_str()
                    .and_then(|m| m.split("codecs=\"").nth(1))
                    .and_then(|c| c.split('"').next())
                    .map(String::from)
            } else {
                Some("none".into())
            };

            let acodec = if mime_type.starts_with("audio/")
                || (vcodec.is_some()
                    && vcodec.as_deref() != Some("none")
                    && item["audioQuality"].is_string())
            {
                item["mimeType"]
                    .as_str()
                    .and_then(|m| m.split("codecs=\"").nth(1))
                    .and_then(|c| c.split('"').next())
                    .map(String::from)
                    .or_else(|| Some("audio".into()))
            } else {
                Some("none".into())
            };

            formats.push(MediaFormat {
                format_id,
                url: final_url,
                ext,
                width,
                height,
                fps,
                vcodec,
                acodec,
                filesize,
                tbr,
                protocol: Some("https".into()),
                http_headers: None,
            });
        }

        formats
    }

    pub async fn fetch_player_js(&self, client: &Client, video_id: &str) -> Option<String> {
        {
            let cache = PLAYER_JS_CACHE.read().await;
            if let Some(ref js) = *cache {
                return Some(js.clone());
            }
        }

        // Fetch watch page to locate base.js
        let watch_url = format!("https://www.youtube.com/watch?v={}", video_id);
        let page_html = client
            .get(&watch_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;

        let js_path = JS_URL_REGEX.find(&page_html)?.as_str();
        let full_js_url = if js_path.starts_with("http") {
            js_path.to_string()
        } else {
            format!("https://www.youtube.com{}", js_path)
        };

        let js_code = client
            .get(&full_js_url)
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;

        let mut cache = PLAYER_JS_CACHE.write().await;
        *cache = Some(js_code.clone());
        Some(js_code)
    }

    /// Checks if format stream URLs or ciphers require JavaScript deciphering or n-challenge solving.
    pub fn requires_js_processing(streaming_data: &Value) -> bool {
        let check_format = |item: &Value| -> bool {
            if item["signatureCipher"].is_string() || item["cipher"].is_string() {
                return true;
            }
            if let Some(url) = item["url"].as_str()
                && (url.contains("n=") || url.contains("&n=") || url.contains("?n="))
            {
                return true;
            }
            false
        };

        if let Some(muxed) = streaming_data["formats"].as_array()
            && muxed.iter().any(check_format)
        {
            return true;
        }
        if let Some(adaptive) = streaming_data["adaptiveFormats"].as_array()
            && adaptive.iter().any(check_format)
        {
            return true;
        }
        false
    }

    /// Transforms the `n` challenge query parameter in stream URL using the player JavaScript decipherer.
    pub fn transform_n_parameter(decipherer: &JsDecipher, js_code: &str, url_str: &str) -> String {
        static N_PARAM_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"(^|[?&])n=([^&]+)"#).expect("Invalid n regex"));

        let Some(caps) = N_PARAM_REGEX.captures(url_str) else {
            return url_str.to_string();
        };

        let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let raw_n = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let decoded_n = urlencoding::decode(raw_n).unwrap_or(std::borrow::Cow::Borrowed(raw_n));

        match decipherer.decipher_n(js_code, &decoded_n) {
            Ok(transformed) => {
                let encoded_transformed = urlencoding::encode(&transformed);
                let target_str = format!("{}n={}", prefix, raw_n);
                let replacement_str = format!("{}n={}", prefix, encoded_transformed);
                url_str.replacen(&target_str, &replacement_str, 1)
            }
            Err(e) => {
                tracing::warn!("Failed to decipher YouTube n-challenge token: {}", e);
                url_str.to_string()
            }
        }
    }

    pub fn parse_media_info_with_js(
        &self,
        json: &Value,
        video_id: &str,
        js_code: Option<&str>,
    ) -> Result<MediaInfo, EngineError> {
        let video_details = &json["videoDetails"];
        if !video_details.is_object() {
            let reason = traverse_string(json, &["playabilityStatus", "reason"])
                .unwrap_or_else(|| "Video unavailable".into());
            return Err(EngineError::StreamNotFound(reason));
        }

        let title = video_details["title"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string();
        let uploader = video_details["author"].as_str().map(String::from);
        let description = video_details["shortDescription"].as_str().map(String::from);
        let duration = video_details["lengthSeconds"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok());

        let thumbnail = traverse_obj(video_details, &["thumbnail", "thumbnails"])
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.last())
            .and_then(|t| t["url"].as_str())
            .map(String::from);

        let decipherer = JsDecipher::new().ok();
        let decipher_param = match (decipherer.as_ref(), js_code) {
            (Some(d), Some(js)) => Some((d, js)),
            _ => None,
        };

        let formats = self.parse_formats(&json["streamingData"], decipher_param);

        let mut subtitles: HashMap<String, Vec<Subtitle>> = HashMap::new();
        if let Some(caption_tracks) = traverse_obj(
            json,
            &[
                "captions",
                "playerCaptionsTracklistRenderer",
                "captionTracks",
            ],
        )
        .and_then(|t| t.as_array())
        {
            for track in caption_tracks {
                if let (Some(lang), Some(url)) =
                    (track["languageCode"].as_str(), track["baseUrl"].as_str())
                {
                    subtitles
                        .entry(lang.to_string())
                        .or_default()
                        .push(Subtitle {
                            ext: "vtt".into(),
                            url: Some(url.to_string()),
                            data: None,
                        });
                }
            }
        }

        Ok(MediaInfo {
            id: video_id.to_string(),
            title,
            description,
            uploader,
            duration,
            thumbnail,
            webpage_url: Some(format!("https://www.youtube.com/watch?v={}", video_id)),
            formats,
            subtitles: if subtitles.is_empty() {
                None
            } else {
                Some(subtitles)
            },
        })
    }

    pub fn parse_media_info(&self, json: &Value, video_id: &str) -> Result<MediaInfo, EngineError> {
        self.parse_media_info_with_js(json, video_id, None)
    }
}

impl Default for YouTubeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncExtractor for YouTubeExtractor {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn matches_url(&self, url: &str) -> bool {
        YOUTUBE_REGEX.is_match(url)
    }

    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaInfo, EngineError> {
        let video_id = Self::extract_video_id(url)
            .ok_or_else(|| EngineError::UnsupportedUrl(format!("Invalid YouTube URL: {}", url)))?;

        let json = self.fetch_player_response(client, &video_id).await?;

        // Check if any formats require cipher deciphering or n-challenge solving
        let requires_js = Self::requires_js_processing(&json["streamingData"]);
        let js_code = if requires_js {
            self.fetch_player_js(client, &video_id).await
        } else {
            None
        };

        self.parse_media_info_with_js(&json, &video_id, js_code.as_deref())
    }
}

#[async_trait]
impl PlatformExtractor for YouTubeExtractor {
    async fn extract_metadata(
        &self,
        client: &Client,
        url: &str,
    ) -> Result<MediaMetadata, EngineError> {
        let video_id = Self::extract_video_id(url)
            .ok_or_else(|| EngineError::UnsupportedUrl(format!("Invalid YouTube URL: {}", url)))?;

        let json = self.fetch_player_response(client, &video_id).await?;
        let video_details = &json["videoDetails"];

        let title = video_details["title"]
            .as_str()
            .unwrap_or("Unknown Title")
            .to_string();
        let artist = video_details["author"]
            .as_str()
            .unwrap_or("Unknown Artist")
            .to_string();
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
            id: video_id.clone(),
            title,
            artist,
            album: None,
            duration_seconds,
            thumbnail_url,
            webpage_url: format!("https://www.youtube.com/watch?v={}", video_id),
        })
    }

    async fn search(
        &self,
        client: &Client,
        query: &str,
    ) -> Result<Vec<MediaMetadata>, EngineError> {
        let body = serde_json::json!({
            "query": query,
            "context": self.build_innertube_context(),
        });

        let resp = client
            .post("https://www.youtube.com/youtubei/v1/search")
            .header("Content-Type", "application/json")
            .header("X-YouTube-Client-Name", "1")
            .header("X-YouTube-Client-Version", "2.20240708.00.00")
            .json(&body)
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let mut results = Vec::new();

        if let Some(contents) = traverse_obj(
            &json,
            &[
                "contents",
                "twoColumnSearchResultsRenderer",
                "primaryContents",
                "sectionListRenderer",
                "contents",
            ],
        )
        .and_then(|c| c.as_array())
        {
            for section in contents {
                if let Some(items) = traverse_obj(section, &["itemSectionRenderer", "contents"])
                    .and_then(|i| i.as_array())
                {
                    for item in items {
                        let render = &item["videoRenderer"];
                        if render.is_object() {
                            let video_id =
                                render["videoId"].as_str().unwrap_or_default().to_string();
                            let title = render["title"]["runs"][0]["text"]
                                .as_str()
                                .unwrap_or("Unknown Title")
                                .to_string();
                            let artist = render["ownerText"]["runs"][0]["text"]
                                .as_str()
                                .unwrap_or("Unknown Artist")
                                .to_string();
                            let webpage_url =
                                format!("https://www.youtube.com/watch?v={}", video_id);

                            results.push(MediaMetadata {
                                id: video_id,
                                title: clean_html(&title),
                                artist: clean_html(&artist),
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

    async fn get_stream_url(
        &self,
        client: &Client,
        track_id: &str,
    ) -> Result<StreamCandidate, EngineError> {
        let json = self.fetch_player_response(client, track_id).await?;

        let requires_js = Self::requires_js_processing(&json["streamingData"]);
        let js_code = if requires_js {
            self.fetch_player_js(client, track_id).await
        } else {
            None
        };

        let decipherer = JsDecipher::new().ok();
        let decipher_param = match (decipherer.as_ref(), js_code.as_deref()) {
            (Some(d), Some(js)) => Some((d, js)),
            _ => None,
        };

        let formats = self.parse_formats(&json["streamingData"], decipher_param);

        let best_audio = formats
            .iter()
            .filter(|f| f.has_audio())
            .max_by(|a, b| {
                let a_tbr = a.tbr.unwrap_or(0.0);
                let b_tbr = b.tbr.unwrap_or(0.0);
                a_tbr
                    .partial_cmp(&b_tbr)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .or_else(|| formats.first())
            .ok_or_else(|| EngineError::StreamNotFound("No stream formats available".into()))?;

        Ok(StreamCandidate {
            format_id: best_audio.format_id.clone(),
            url: best_audio.url.clone(),
            ext: best_audio.ext.clone(),
            resolution: best_audio.height.map(|h| format!("{}p", h)),
            filesize_bytes: best_audio.filesize,
            tbr_kbps: best_audio.tbr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_video_id() {
        assert_eq!(
            YouTubeExtractor::extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            YouTubeExtractor::extract_video_id("https://youtu.be/dQw4w9WgXcQ?t=42"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            YouTubeExtractor::extract_video_id("https://www.youtube.com/shorts/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            YouTubeExtractor::extract_video_id("https://www.youtube.com/embed/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            YouTubeExtractor::extract_video_id("https://www.youtube.com/live/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            YouTubeExtractor::extract_video_id("https://example.com/not_youtube"),
            None
        );
    }

    #[test]
    fn test_parse_player_response_mock() {
        let mock_json = json!({
            "videoDetails": {
                "videoId": "dQw4w9WgXcQ",
                "title": "Never Gonna Give You Up",
                "author": "Rick Astley",
                "lengthSeconds": "213",
                "shortDescription": "Official music video",
                "thumbnail": {
                    "thumbnails": [
                        {"url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg", "width": 120, "height": 90},
                        {"url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg", "width": 1280, "height": 720}
                    ]
                }
            },
            "streamingData": {
                "formats": [
                    {
                        "itag": 18,
                        "url": "https://rr.googlevideo.com/videoplayback?itag=18",
                        "mimeType": "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"",
                        "width": 640,
                        "height": 360,
                        "fps": 30.0,
                        "bitrate": 500000,
                        "contentLength": "15000000"
                    }
                ],
                "adaptiveFormats": [
                    {
                        "itag": 137,
                        "url": "https://rr.googlevideo.com/videoplayback?itag=137",
                        "mimeType": "video/mp4; codecs=\"avc1.640028\"",
                        "width": 1920,
                        "height": 1080,
                        "fps": 30.0,
                        "bitrate": 4000000,
                        "contentLength": "120000000"
                    },
                    {
                        "itag": 251,
                        "url": "https://rr.googlevideo.com/videoplayback?itag=251",
                        "mimeType": "audio/webm; codecs=\"opus\"",
                        "bitrate": 160000,
                        "contentLength": "4000000"
                    }
                ]
            }
        });

        let extractor = YouTubeExtractor::new();
        let info = extractor
            .parse_media_info(&mock_json, "dQw4w9WgXcQ")
            .expect("Failed to parse mock MediaInfo");

        assert_eq!(info.id, "dQw4w9WgXcQ");
        assert_eq!(info.title, "Never Gonna Give You Up");
        assert_eq!(info.uploader.as_deref(), Some("Rick Astley"));
        assert_eq!(info.duration, Some(213));
        assert_eq!(info.formats.len(), 3);

        let best_v = info.best_video().expect("No video format found");
        assert_eq!(best_v.format_id, "137");
        assert_eq!(best_v.height, Some(1080));

        let best_a = info.best_audio().expect("No audio format found");
        assert_eq!(best_a.format_id, "251");
        assert_eq!(best_a.ext, "webm");
    }

    #[test]
    fn test_parse_formats_with_cipher() {
        let mock_streaming_data = json!({
            "adaptiveFormats": [
                {
                    "itag": 251,
                    "signatureCipher": "s=abcdefg&sp=sig&url=https%3A%2F%2Frr.googlevideo.com%2Fvideoplayback%3Fitag%3D251",
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "bitrate": 160000
                }
            ]
        });

        let mock_js = r#"
            var helperObj = {
                reverse: function(a) { a.reverse(); },
                swap: function(a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c; },
                splice: function(a, b) { a.splice(0, b); }
            };
            var decipherFn = function(a) {
                var b = a.split("");
                helperObj.reverse(b);
                helperObj.swap(b, 2);
                return b.join("");
            };
        "#;

        let decipherer = JsDecipher::new().expect("Failed to initialize JsDecipher");
        let extractor = YouTubeExtractor::new();
        let formats = extractor.parse_formats(&mock_streaming_data, Some((&decipherer, mock_js)));

        assert_eq!(formats.len(), 1);
        let format = &formats[0];
        assert_eq!(format.format_id, "251");
        assert!(format.url.contains("sig=efgdcba"));
    }

    #[test]
    fn test_parse_formats_with_n_challenge() {
        let mock_streaming_data = json!({
            "formats": [
                {
                    "itag": 18,
                    "url": "https://rr1.googlevideo.com/videoplayback?itag=18&n=12345&expire=9999",
                    "mimeType": "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"",
                    "bitrate": 500000
                }
            ],
            "adaptiveFormats": [
                {
                    "itag": 251,
                    "signatureCipher": "s=abcdefg&sp=sig&url=https%3A%2F%2Frr.googlevideo.com%2Fvideoplayback%3Fitag%3D251%26n%3D12345",
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "bitrate": 160000
                }
            ]
        });

        let mock_js = r#"
            var helperObj = {
                reverse: function(a) { a.reverse(); },
                swap: function(a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c; },
                splice: function(a, b) { a.splice(0, b); }
            };
            var decipherFn = function(a) {
                var b = a.split("");
                helperObj.reverse(b);
                helperObj.swap(b, 2);
                return b.join("");
            };
            var nFunc = function(a) {
                var b = a.split("");
                helperObj.reverse(b);
                helperObj.swap(b, 1);
                return b.join("");
            };
            var entry = a.get("n") && (b = nFunc(b));
        "#;

        let decipherer = JsDecipher::new().expect("Failed to initialize JsDecipher");
        let extractor = YouTubeExtractor::new();
        let formats = extractor.parse_formats(&mock_streaming_data, Some((&decipherer, mock_js)));

        assert_eq!(formats.len(), 2);
        // Direct format with n transformed from "12345" -> "45321"
        let fmt18 = formats
            .iter()
            .find(|f| f.format_id == "18")
            .expect("format 18");
        assert!(fmt18.url.contains("n=45321"));
        assert!(!fmt18.url.contains("n=12345"));

        // Cipher format with both sig=efgdcba and n transformed from "12345" -> "45321"
        let fmt251 = formats
            .iter()
            .find(|f| f.format_id == "251")
            .expect("format 251");
        assert!(fmt251.url.contains("sig=efgdcba"));
        assert!(fmt251.url.contains("n=45321"));
        assert!(!fmt251.url.contains("n=12345"));
    }
}
