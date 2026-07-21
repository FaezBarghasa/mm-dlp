//! Utility functions for URL sanitization, platform detection, filename cleaning, and path validation.

use crate::error::EngineError;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use url::Url;

static SPOTIFY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)spotify\.com").expect("Invalid regex for spotify"));
static SOUNDCLOUD_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)soundcloud\.com").expect("Invalid regex for soundcloud"));
static YOUTUBE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(youtube\.com|youtu\.be)").expect("Invalid regex for youtube"));

/// Strips query params, fragments, and trailing slashes using the `url` crate.
pub fn sanitize_url(raw_url: &str) -> Result<String, EngineError> {
    let mut parsed = Url::parse(raw_url.trim())
        .map_err(|e| EngineError::UnsupportedUrl(format!("Invalid URL syntax: {}", e)))?;
    parsed.set_query(None);
    parsed.set_fragment(None);

    let mut clean_str = parsed.to_string();
    if clean_str.len() > 1 && clean_str.ends_with('/') {
        clean_str.pop();
    }
    Ok(clean_str)
}

/// Detects the platform (e.g. "spotify", "soundcloud", "youtube") from URL using `std::sync::LazyLock` regexes.
pub fn detect_platform(raw_url: &str) -> Option<&'static str> {
    if SPOTIFY_REGEX.is_match(raw_url) {
        Some("spotify")
    } else if SOUNDCLOUD_REGEX.is_match(raw_url) {
        Some("soundcloud")
    } else if YOUTUBE_REGEX.is_match(raw_url) {
        Some("youtube")
    } else {
        None
    }
}

/// Replaces illegal OS characters (`<>:"/\|?*`), replaces non-ASCII control characters, and truncates to 200 chars.
pub fn sanitize_filename(filename: &str) -> String {
    let sanitized: String = filename
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    if sanitized.chars().count() > 200 {
        sanitized.chars().take(200).collect()
    } else {
        sanitized
    }
}

/// Formats byte sizes to human-readable strings (B, KB, MB, GB).
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Resolves file path collisions by appending `(1)`, `(2)`, etc. if target file already exists.
pub fn resolve_duplicate_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();

    let mut counter = 1;
    loop {
        let new_name = format!("{} ({}){}", stem, counter, extension);
        let candidate = parent.join(new_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Validates target output path strictly to prevent path traversal attacks (`..` components) and empty paths.
pub fn validate_output_path(output_path: &str) -> Result<PathBuf, EngineError> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        return Err(EngineError::InvalidConfig(
            "Output path cannot be empty".to_string(),
        ));
    }

    let path = Path::new(trimmed);
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(EngineError::InvalidConfig(
                "Path traversal ('..') is strictly forbidden".to_string(),
            ));
        }
    }

    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_url() {
        let raw = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=shared#t=10/";
        let clean = sanitize_url(raw).unwrap();
        assert_eq!(clean, "https://www.youtube.com/watch");
    }

    #[test]
    fn test_detect_platform() {
        assert_eq!(
            detect_platform("https://open.spotify.com/track/123"),
            Some("spotify")
        );
        assert_eq!(
            detect_platform("https://soundcloud.com/artist/track"),
            Some("soundcloud")
        );
        assert_eq!(
            detect_platform("https://youtu.be/dQw4w9WgXcQ"),
            Some("youtube")
        );
        assert_eq!(detect_platform("https://example.com"), None);
    }

    #[test]
    fn test_sanitize_filename() {
        let invalid = "invalid:file/name?with*chars<>";
        assert_eq!(sanitize_filename(invalid), "invalid_file_name_with_chars__");
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(2048), "2.00 KB");
        assert_eq!(format_file_size(10_485_760), "10.00 MB");
    }

    #[test]
    fn test_validate_output_path() {
        assert!(validate_output_path("").is_err());
        assert!(validate_output_path("../forbidden/path").is_err());
        assert!(validate_output_path("downloads/music/song.mp3").is_ok());
    }
}
