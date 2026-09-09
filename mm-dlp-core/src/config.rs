use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct EngineConfig {
    pub max_concurrent_downloads: u32,
    pub user_agent: String,
    pub timeout_seconds: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 4,
            user_agent: String::from("mm-dlp/1.0.0"),
            timeout_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct DownloadConfig {
    pub output_dir: String,
    pub filename_template: String,
    pub extract_audio: bool,
    pub audio_format: String,
    pub quality: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct MediaMetadata {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_seconds: u64,
    pub thumbnail_url: Option<String>,
    pub webpage_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct StreamCandidate {
    pub format_id: String,
    pub url: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub filesize_bytes: Option<u64>,
    pub tbr_kbps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct DownloadResult {
    pub file_path: String,
    pub total_bytes: u64,
    pub elapsed_millis: u64,
}
