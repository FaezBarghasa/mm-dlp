use thiserror::Error;

/// The central unified error type for `mm-dlp-core`.
#[derive(Debug, Clone, Error, uniffi::Error)]
pub enum EngineError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("Unsupported URL: {0}")]
    UnsupportedUrl(String),

    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    #[error("FFmpeg error: {0}")]
    FfmpegError(String),

    #[error("FFmpeg not found in PATH")]
    FfmpegNotFound,

    #[error("Tagging error: {0}")]
    TaggingError(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("QUIC fallback exhausted: {0}")]
    QuicFallbackExhausted(String),
}

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        EngineError::IoError(err.to_string())
    }
}

impl From<reqwest::Error> for EngineError {
    fn from(err: reqwest::Error) -> Self {
        EngineError::Network(err.to_string())
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(err: serde_json::Error) -> Self {
        EngineError::Serialization(err.to_string())
    }
}