use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use std::sync::LazyLock;

use super::traits::{AsyncExtractor, MediaFormat, MediaInfo};
use crate::error::EngineError;

static DIRECT_MEDIA_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(mp4|webm|mkv|m4a|mp3|flac|wav|ogg|opus|m3u8|mpd)(?:\?.*)?$")
        .expect("Invalid media regex")
});

static OG_TITLE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<meta\s+property=["']og:title["']\s+content=["']([^"']+)["']"#)
        .expect("Invalid title regex")
});

static VIDEO_TAG_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<video[^>]*src=["']([^"']+)["']"#).expect("Invalid video tag regex")
});

static SOURCE_TAG_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<source[^>]*src=["']([^"']+)["'](?:[^>]*type=["']([^"']+)["'])?"#)
        .expect("Invalid source tag regex")
});

pub struct GenericExtractor;

impl GenericExtractor {
    pub fn new() -> Self {
        Self
    }

    fn deduce_ext_and_protocol(url: &str) -> (String, Option<String>) {
        let clean = url.split('?').next().unwrap_or(url);
        if clean.ends_with(".m3u8") {
            ("mp4".into(), Some("m3u8_native".into()))
        } else if clean.ends_with(".mpd") {
            ("mp4".into(), Some("http_dash_segments".into()))
        } else if let Some(dot_idx) = clean.rfind('.') {
            let ext = &clean[dot_idx + 1..];
            if ext.len() <= 5 && ext.chars().all(|c| c.is_alphanumeric()) {
                (ext.to_lowercase(), None)
            } else {
                ("mp4".into(), None)
            }
        } else {
            ("mp4".into(), None)
        }
    }

    fn extract_filename_title(url: &str) -> String {
        let clean = url.split('?').next().unwrap_or(url);
        clean
            .rsplit('/')
            .next()
            .and_then(|f| f.rsplit_once('.').map(|(base, _)| base))
            .filter(|s| !s.is_empty())
            .unwrap_or("media")
            .replace(['_', '-'], " ")
    }
}

impl Default for GenericExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AsyncExtractor for GenericExtractor {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn matches_url(&self, url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    async fn extract_metadata(&self, client: &Client, url: &str) -> Result<MediaInfo, EngineError> {
        // 1. Direct media URL check
        if DIRECT_MEDIA_REGEX.is_match(url) {
            let (ext, protocol) = Self::deduce_ext_and_protocol(url);
            let title = Self::extract_filename_title(url);
            let id = format!("{:x}", md5_hash(url));

            let format = MediaFormat {
                format_id: "direct".into(),
                url: url.to_string(),
                ext,
                width: None,
                height: None,
                fps: None,
                vcodec: Some("unknown".into()),
                acodec: Some("unknown".into()),
                filesize: None,
                tbr: None,
                protocol,
                http_headers: None,
            };

            return Ok(MediaInfo {
                id,
                title,
                description: None,
                uploader: None,
                duration: None,
                thumbnail: None,
                webpage_url: Some(url.to_string()),
                formats: vec![format],
                subtitles: None,
            });
        }

        // 2. Fetch page and inspect HTML5 tags
        let response =
            client.get(url).send().await.map_err(|e| {
                EngineError::Network(format!("Failed to fetch generic page: {}", e))
            })?;

        let html = response
            .text()
            .await
            .map_err(|e| EngineError::Network(format!("Failed to read page body: {}", e)))?;

        let title = OG_TITLE_REGEX
            .captures(&html)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .unwrap_or_else(|| Self::extract_filename_title(url));

        let mut formats = Vec::new();
        let mut idx = 1;

        for cap in VIDEO_TAG_REGEX.captures_iter(&html) {
            if let Some(src) = cap.get(1) {
                let media_url = resolve_relative_url(url, src.as_str());
                let (ext, protocol) = Self::deduce_ext_and_protocol(&media_url);
                formats.push(MediaFormat {
                    format_id: format!("video-{}", idx),
                    url: media_url,
                    ext,
                    width: None,
                    height: None,
                    fps: None,
                    vcodec: Some("unknown".into()),
                    acodec: Some("unknown".into()),
                    filesize: None,
                    tbr: None,
                    protocol,
                    http_headers: None,
                });
                idx += 1;
            }
        }

        for cap in SOURCE_TAG_REGEX.captures_iter(&html) {
            if let Some(src) = cap.get(1) {
                let media_url = resolve_relative_url(url, src.as_str());
                let (ext, protocol) = Self::deduce_ext_and_protocol(&media_url);
                formats.push(MediaFormat {
                    format_id: format!("source-{}", idx),
                    url: media_url,
                    ext,
                    width: None,
                    height: None,
                    fps: None,
                    vcodec: Some("unknown".into()),
                    acodec: Some("unknown".into()),
                    filesize: None,
                    tbr: None,
                    protocol,
                    http_headers: None,
                });
                idx += 1;
            }
        }

        if formats.is_empty() {
            return Err(EngineError::ExtractorError(format!(
                "No media sources found on webpage: {}",
                url
            )));
        }

        let id = format!("{:x}", md5_hash(url));
        Ok(MediaInfo {
            id,
            title,
            description: None,
            uploader: None,
            duration: None,
            thumbnail: None,
            webpage_url: Some(url.to_string()),
            formats,
            subtitles: None,
        })
    }
}

fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if relative.starts_with("//") {
        return format!("https:{}", relative);
    }
    if let Ok(base_url) = url::Url::parse(base)
        && let Ok(joined) = base_url.join(relative)
    {
        return joined.to_string();
    }
    relative.to_string()
}

fn md5_hash(data: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_media_detection() {
        let ext = GenericExtractor::new();
        assert!(ext.matches_url("https://example.com/stream.mp4"));
        assert!(ext.matches_url("https://example.com/playlist.m3u8"));
        assert!(ext.matches_url("https://example.com/manifest.mpd"));
    }

    #[tokio::test]
    async fn test_direct_mp4_metadata() {
        let ext = GenericExtractor::new();
        let client = Client::new();
        let info = ext
            .extract_metadata(&client, "https://example.com/videos/sample.mp4")
            .await
            .unwrap();

        assert_eq!(info.title, "sample");
        assert_eq!(info.formats.len(), 1);
        assert_eq!(info.formats[0].ext, "mp4");
        assert_eq!(info.formats[0].url, "https://example.com/videos/sample.mp4");
    }
}
