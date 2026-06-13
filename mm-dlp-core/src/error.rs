use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum EngineError {
    #[error("Network operation failed: {0}")]
    Network(String),

    #[error("I/O operation failed: {0}")]
    Io(String),

    #[error("Data parsing or serialization failed: {0}")]
    Parsing(String),
}

impl From<reqwest::Error> for EngineError {
    fn from(error: reqwest::Error) -> Self {
        EngineError::Network(error.to_string())
    }
}

impl From<std::io::Error> for EngineError {
    fn from(error: std::io::Error) -> Self {
        EngineError::Io(error.to_string())
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(error: serde_json::Error) -> Self {
        EngineError::Parsing(error.to_string())
    }
}