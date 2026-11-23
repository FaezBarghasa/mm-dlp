#![allow(clippy::double_must_use)]
//! # mm-dlp-core
//!
//! High-performance media extraction & downloading engine for Android via UniFFI.

uniffi::setup_scaffolding!();

pub mod client;
pub mod config;
pub mod downloader;
pub mod engine;
pub mod error;
pub mod extractor;
pub mod js;
pub mod playlist;
pub mod processor;
pub mod resolver;
pub mod utils;

pub use config::{DownloadConfig, DownloadResult, EngineConfig, MediaMetadata, StreamCandidate};
pub use engine::{DownloadProgressCallback, MmDlpEngine};
pub use error::EngineError;
pub use playlist::{Playlist, PlaylistTrack, slice_playlist_items};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniffi_scaffolding_init() {
        let config = EngineConfig::default();
        assert_eq!(config.max_concurrent_downloads, 4);
        assert_eq!(config.timeout_seconds, 30);
    }
}
