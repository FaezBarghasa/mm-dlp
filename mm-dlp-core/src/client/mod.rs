pub mod cookies;
pub mod h2;
pub mod tls;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("File system error: {0}")]
    FileSystemError(String),
    #[error("Decryption error: {0}")]
    DecryptionError(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("OS API error: {0}")]
    OsApiError(String),
}