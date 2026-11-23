use serde::{Deserialize, Serialize};

pub const FIREFOX_GECKO_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:135.0) Gecko/20100101 Firefox/135.0";
pub const FIREFOX_LINUX_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:135.0) Gecko/20100101 Firefox/135.0";
pub const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";
pub const SAFARI_MACOS_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15";
pub const SAFARI_IOS_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_3 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Mobile/15E148 Safari/604.1";

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct EngineConfig {
    pub max_concurrent_downloads: u32,
    pub user_agent: String,
    pub timeout_seconds: u64,
    pub proxy: Option<String>,
    pub cookies_file: Option<String>,
    pub buffer_size_bytes: Option<u64>,
    pub cookies_from_browser: Option<String>,
    pub impersonate_target: Option<String>,
    pub dns_servers: Option<String>,
    pub dangerously_insecure: bool,
    pub custom_headers: Option<Vec<String>>,
}

impl EngineConfig {
    pub fn is_gecko_or_firefox(&self) -> bool {
        if let Some(ref imp) = self.impersonate_target {
            let lower = imp.to_lowercase();
            if lower.contains("firefox")
                || lower.contains("gecko")
                || lower.contains("librewolf")
                || lower.contains("waterfox")
            {
                return true;
            }
        }
        if let Some(ref b) = self.cookies_from_browser {
            let lower = b.to_lowercase();
            if lower.contains("firefox")
                || lower.contains("gecko")
                || lower.contains("librewolf")
                || lower.contains("waterfox")
            {
                return true;
            }
        }
        false
    }

    pub fn is_safari_or_webkit(&self) -> bool {
        if let Some(ref imp) = self.impersonate_target {
            let lower = imp.to_lowercase();
            if lower.contains("safari")
                || lower.contains("webkit")
                || lower.contains("apple")
                || lower.contains("ios")
                || lower.contains("macos")
            {
                return true;
            }
        }
        if let Some(ref b) = self.cookies_from_browser {
            let lower = b.to_lowercase();
            if lower.contains("safari") || lower.contains("webkit") || lower.contains("apple") {
                return true;
            }
        }
        false
    }

    pub fn is_ios_safari(&self) -> bool {
        #[cfg(target_os = "ios")]
        return true;
        #[cfg(not(target_os = "ios"))]
        {
            if let Some(ref imp) = self.impersonate_target {
                let lower = imp.to_lowercase();
                if lower.contains("ios") || lower.contains("iphone") || lower.contains("ipad") {
                    return true;
                }
            }
            false
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 4,
            user_agent: String::from("mm-dlp/1.0.0"),
            timeout_seconds: 30,
            proxy: None,
            cookies_file: None,
            buffer_size_bytes: None,
            cookies_from_browser: None,
            impersonate_target: None,
            dns_servers: None,
            dangerously_insecure: false,
            custom_headers: None,
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
