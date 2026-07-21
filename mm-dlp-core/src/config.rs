//! Configuration data structures for the engine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(uniffi::Record)]
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
