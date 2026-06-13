use thiserror::Error;

/// The central error type used throughout the `mm-dlp-core` engine.
#[derive(Debug, Error, uniffi::Error)]
pub enum EngineError {
    /// An error related to network operations (e.g., fetching a URL).
    #[error("Network operation failed: {0}")]
    Network(String),

    /// An error related to input/output operations (e.g., writing a file).
    #[error("I/O operation failed: {0}")]
    Io(String),

    /// An error related to parsing or serialization (e.g., parsing JSON).
    #[error("Data parsing or serialization failed: {0}")]
    Parsing(String),
}

/// Allows automatic conversion from `reqwest::Error` to `EngineError::Network`.
impl From<reqwest::Error> for EngineError {
    fn from(error: reqwest::Error) -> Self {
        EngineError::Network(error.to_string())
    }
}

/// Allows automatic conversion from `std::io::Error` to `EngineError::Io`.
impl From<std::io::Error> for EngineError {
    fn from(error: std::io::Error) -> Self {
        EngineError::Io(error.to_string())
    }
}

/// Allows automatic conversion from `serde_json::Error` to `EngineError::Parsing`.
impl From<serde_json::Error> for EngineError {
    fn from(error: serde_json::Error) -> Self {
        EngineError::Parsing(error.to_string())
    }
}