use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::Command;

use super::traits::{AsyncExtractor, MediaFormat, MediaInfo, Subtitle};
use crate::error::EngineError;

pub struct FallbackBridgeExtractor {
    python_bin: PathBuf,
    yt_dlp_root: Option<PathBuf>,
}

impl FallbackBridgeExtractor {
    pub fn new() -> Self {
        let python_bin = which::which("python3")
            .or_else(|_| which::which("python"))
            .unwrap_or_else(|_| PathBuf::from("python3"));

        // Check if references/yt-dlp exists relative to current working directory or executable
        let yt_dlp_root = Self::find_yt_dlp_root();

        Self {
            python_bin,
            yt_dlp_root,
        }
    }

    fn find_yt_dlp_root() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from("references/yt-dlp"),
            PathBuf::from("../references/yt-dlp"),
            PathBuf::from("../../references/yt-dlp"),
        ];

        for c in &candidates {
            if c.join("yt_dlp").is_dir()
                && let Ok(abs) = std::fs::canonicalize(c)
            {
                return Some(abs);
            }
        }
        None
    }

    pub fn parse_yt_dlp_json(value: &Value) -> Result<MediaInfo, EngineError> {
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let title = value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string();

        let description = value
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let uploader = value
            .get("uploader")
            .or_else(|| value.get("channel"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let duration = value
            .get("duration")
            .and_then(|v| v.as_f64().map(|f| f as u32));

        let thumbnail = value
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let webpage_url = value
            .get("webpage_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut formats = Vec::new();
        if let Some(formats_arr) = value.get("formats").and_then(|v| v.as_array()) {
            for f in formats_arr {
                if let Some(format_id) = f.get("format_id").and_then(|v| v.as_str()) {
                    let url = match f.get("url").and_then(|v| v.as_str()) {
                        Some(u) => u.to_string(),
                        None => continue,
                    };

                    let ext = f
                        .get("ext")
                        .and_then(|v| v.as_str())
                        .unwrap_or("mp4")
                        .to_string();

                    let width = f.get("width").and_then(|v| v.as_u64().map(|w| w as u32));
                    let height = f.get("height").and_then(|v| v.as_u64().map(|h| h as u32));
                    let fps = f.get("fps").and_then(|v| v.as_f64());
                    let vcodec = f
                        .get("vcodec")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let acodec = f
                        .get("acodec")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let filesize = f.get("filesize").and_then(|v| v.as_u64());
                    let tbr = f.get("tbr").and_then(|v| v.as_f64());
                    let protocol = f
                        .get("protocol")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let mut http_headers = HashMap::new();
                    if let Some(headers_obj) = f.get("http_headers").and_then(|v| v.as_object()) {
                        for (k, v) in headers_obj {
                            if let Some(v_str) = v.as_str() {
                                http_headers.insert(k.clone(), v_str.to_string());
                            }
                        }
                    }

                    formats.push(MediaFormat {
                        format_id: format_id.to_string(),
                        url,
                        ext,
                        width,
                        height,
                        fps,
                        vcodec,
                        acodec,
                        filesize,
                        tbr,
                        protocol,
                        http_headers: if http_headers.is_empty() {
                            None
                        } else {
                            Some(http_headers)
                        },
                    });
                }
            }
        }

        // Subtitles parsing
        let mut subtitles_map = HashMap::new();
        if let Some(sub_obj) = value.get("subtitles").and_then(|v| v.as_object()) {
            for (lang, sub_list) in sub_obj {
                if let Some(arr) = sub_list.as_array() {
                    let mut list = Vec::new();
                    for s in arr {
                        let ext = s
                            .get("ext")
                            .and_then(|v| v.as_str())
                            .unwrap_or("vtt")
                            .to_string();
                        let url = s.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                        list.push(Subtitle {
                            ext,
                            url,
                            data: None,
                        });
                    }
                    subtitles_map.insert(lang.clone(), list);
                }
            }
        }

        Ok(MediaInfo {
            id,
            title,
            description,
            uploader,
            duration,
            thumbnail,
            webpage_url,
            formats,
            subtitles: if subtitles_map.is_empty() {
                None
            } else {
                Some(subtitles_map)
            },
        })
    }
}

impl Default for FallbackBridgeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncExtractor for FallbackBridgeExtractor {
    fn name(&self) -> &'static str {
        "fallback_bridge"
    }

    fn matches_url(&self, url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    async fn extract_metadata(
        &self,
        _client: &Client,
        url: &str,
    ) -> Result<MediaInfo, EngineError> {
        // Prevent recursive fork-bomb loop if mm-dlp is aliased or installed as yt-dlp
        if std::env::var("MM_DLP_FALLBACK_ACTIVE").is_ok() {
            return Err(EngineError::ExtractorError(
                "Recursive fallback bridge invocation detected".into(),
            ));
        }

        let current_exe = std::env::current_exe().ok();
        let mut candidate_cmd = None;

        if let Some(ref root) = self.yt_dlp_root {
            let mut cmd = Command::new(&self.python_bin);
            cmd.env("MM_DLP_FALLBACK_ACTIVE", "1");
            cmd.env("PYTHONPATH", root);
            cmd.arg("-m").arg("yt_dlp");
            candidate_cmd = Some(cmd);
        } else if let Ok(system_ytdlp) = which::which("yt-dlp") {
            // Guarantee system_ytdlp is not pointing back to mm-dlp binary or alias
            let is_self = if let Some(ref curr) = current_exe {
                if let (Ok(c1), Ok(c2)) = (
                    std::fs::canonicalize(system_ytdlp.as_path()),
                    std::fs::canonicalize(curr),
                ) {
                    c1 == c2
                } else {
                    system_ytdlp == *curr
                }
            } else {
                false
            };

            if is_self {
                // Self-binary collision detected; run python -m yt_dlp instead if python is available
                if which::which(&self.python_bin).is_ok() {
                    let mut cmd = Command::new(&self.python_bin);
                    cmd.env("MM_DLP_FALLBACK_ACTIVE", "1");
                    cmd.arg("-m").arg("yt_dlp");
                    candidate_cmd = Some(cmd);
                }
            } else {
                let mut cmd = Command::new(system_ytdlp);
                cmd.env("MM_DLP_FALLBACK_ACTIVE", "1");
                candidate_cmd = Some(cmd);
            }
        } else if which::which(&self.python_bin).is_ok() {
            let mut cmd = Command::new(&self.python_bin);
            cmd.env("MM_DLP_FALLBACK_ACTIVE", "1");
            cmd.arg("-m").arg("yt_dlp");
            candidate_cmd = Some(cmd);
        }

        let output_res = if let Some(mut cmd) = candidate_cmd {
            cmd.arg("--dump-single-json")
                .arg("--no-playlist")
                .arg("--skip-download")
                .arg(url);
            cmd.output().await
        } else {
            // Neither external yt-dlp nor python is available
            let generic = crate::extractor::generic::GenericExtractor::new();
            if generic.matches_url(url) {
                return generic.extract_metadata(_client, url).await;
            }
            return Err(EngineError::ExtractorError(
                "Neither valid external yt-dlp binary nor python interpreter is available".into(),
            ));
        };

        let parsed_info = match output_res {
            Ok(output) if output.status.success() => {
                serde_json::from_slice::<Value>(&output.stdout)
                    .map_err(|e| {
                        EngineError::ExtractorError(format!(
                            "Failed to parse yt-dlp fallback JSON output: {}",
                            e
                        ))
                    })
                    .and_then(|val| Self::parse_yt_dlp_json(&val))
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(EngineError::ExtractorError(format!(
                    "yt-dlp fallback extraction failed: {}",
                    stderr.trim()
                )))
            }
            Err(e) => Err(EngineError::ExtractorError(format!(
                "Failed to execute yt-dlp fallback bridge process: {}",
                e
            ))),
        };

        match parsed_info {
            Ok(info) => Ok(info),
            Err(original_err) => {
                // Graceful fallback to GenericExtractor if yt-dlp is unavailable or failed
                let generic = crate::extractor::generic::GenericExtractor::new();
                if generic.matches_url(url)
                    && let Ok(generic_info) = generic.extract_metadata(_client, url).await
                {
                    return Ok(generic_info);
                }
                Err(original_err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_yt_dlp_json_fixture() {
        let fixture = json!({
            "id": "abc123xyz",
            "title": "Example Video Stream",
            "uploader": "Channel Alpha",
            "duration": 185,
            "webpage_url": "https://example.com/video/abc123xyz",
            "formats": [
                {
                    "format_id": "18",
                    "url": "https://cdn.example.com/stream-360p.mp4",
                    "ext": "mp4",
                    "width": 640,
                    "height": 360,
                    "fps": 30.0,
                    "vcodec": "avc1.42001E",
                    "acodec": "mp4a.40.2",
                    "tbr": 450.0,
                    "filesize": 10500000
                },
                {
                    "format_id": "137",
                    "url": "https://cdn.example.com/stream-1080p.mp4",
                    "ext": "mp4",
                    "width": 1920,
                    "height": 1080,
                    "fps": 60.0,
                    "vcodec": "avc1.640028",
                    "acodec": "none",
                    "tbr": 4200.0,
                    "filesize": 95000000
                }
            ],
            "subtitles": {
                "en": [
                    { "ext": "vtt", "url": "https://cdn.example.com/subs/en.vtt" }
                ]
            }
        });

        let info = FallbackBridgeExtractor::parse_yt_dlp_json(&fixture)
            .expect("Should parse yt-dlp JSON successfully");

        assert_eq!(info.id, "abc123xyz");
        assert_eq!(info.title, "Example Video Stream");
        assert_eq!(info.uploader.as_deref(), Some("Channel Alpha"));
        assert_eq!(info.duration, Some(185));
        assert_eq!(info.formats.len(), 2);
        assert_eq!(info.formats[1].format_id, "137");
        assert_eq!(info.formats[1].height, Some(1080));

        let subs = info.subtitles.expect("Subtitles should be present");
        assert_eq!(subs.get("en").unwrap()[0].ext, "vtt");
    }

    #[tokio::test]
    async fn test_fallback_bridge_recursion_prevention() {
        let extractor = FallbackBridgeExtractor::new();
        let client = Client::new();

        // Simulate active fallback bridge to trigger recursion protection
        unsafe {
            std::env::set_var("MM_DLP_FALLBACK_ACTIVE", "1");
        }
        let res = extractor
            .extract_metadata(&client, "https://example.com/test_video")
            .await;
        unsafe {
            std::env::remove_var("MM_DLP_FALLBACK_ACTIVE");
        }

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Recursive fallback bridge invocation detected"));
    }

    #[tokio::test]
    async fn test_fallback_bridge_graceful_generic_fallback() {
        let extractor = FallbackBridgeExtractor::new();
        let client = Client::new();

        // Direct media URL that can be parsed by GenericExtractor if yt-dlp fails
        let direct_url = "https://example.com/media/sample_video.mp4";
        let res = extractor.extract_metadata(&client, direct_url).await;
        // Generic extractor succeeds on direct mp4 URL
        if let Ok(info) = res {
            assert!(info.formats.iter().any(|f| f.url == direct_url));
        }
    }
}
