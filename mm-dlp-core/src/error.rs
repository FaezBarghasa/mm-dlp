use thiserror::Error;

/// The central error type used throughout the `mm-dlp-core` engine.
#[derive(Debug, Clone, Error, uniffi::Error)]
pub enum EngineError {
    /// An error related to the file system.
    #[error("File system error: {0}")]
    FileSystemError(String),

    /// An error related to decryption.
    #[error("Decryption error: {0}")]
    DecryptionError(String),

    /// An error related to database operations.
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// An error related to OS APIs.
    #[error("OS API error: {0}")]
    OsApiError(String),

    /// An error related to network operations (e.g., fetching a URL).
    #[error("Network operation failed: {0}")]
    Network(String),

    /// An error related to input/output operations (e.g., writing a file).
    #[error("I/O operation failed: {0}")]
    Io(String),

    /// An error related to parsing or serialization (e.g., parsing JSON).
    #[error("Data parsing or serialization failed: {0}")]
    Parsing(String),

    /// The `ffmpeg` binary was not found in the system PATH.
    #[error("ffmpeg not found in PATH; please install ffmpeg")]
    FfmpegNotFound,

    /// A media processing operation (tagging, conversion) failed.
    #[error("Media processing error: {0}")]
    MediaError(String),
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

/// Allows automatic conversion from `anyhow::Error` to `EngineError::OsApiError`.
impl From<anyhow::Error> for EngineError {
    fn from(error: anyhow::Error) -> Self {
        EngineError::OsApiError(error.to_string())
    }
}