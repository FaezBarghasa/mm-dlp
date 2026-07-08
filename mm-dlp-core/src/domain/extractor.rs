use thiserror::Error;

/// Domain-level error type for all platform extractors.
/// Separate from `EngineError` so extractors are decoupled from the FFI layer.
#[derive(Debug, Error, Clone)]
pub enum ExtractorError {
    /// The URL did not match any known platform pattern.
    #[error("Unsupported URL: {0}")]
    UnsupportedUrl(String),

    /// A network request failed.
    #[error("Network error: {0}")]
    Network(String),

    /// The API response could not be parsed.
    #[error("Parse error: {0}")]
    Parse(String),

    /// No suitable audio stream was found for the requested quality.
    #[error("No audio stream found: {0}")]
    NoStream(String),

    /// Platform-specific authentication failed.
    #[error("Auth error: {0}")]
    Auth(String),

    /// Rate-limited by the platform.
    #[error("Rate limited: {0}")]
    RateLimited(String),
}

impl From<ExtractorError> for anyhow::Error {
    fn from(e: ExtractorError) -> Self {
        anyhow::anyhow!("{}", e)
    }
}

impl From<reqwest::Error> for ExtractorError {
    fn from(e: reqwest::Error) -> Self {
        ExtractorError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for ExtractorError {
    fn from(e: serde_json::Error) -> Self {
        ExtractorError::Parse(e.to_string())
    }
}
