use mm_dlp_core::error::EngineError;
use std::io::{Error as IoError, ErrorKind};

#[test]
fn test_io_error_conversion() {
    let io_err = IoError::new(ErrorKind::PermissionDenied, "access denied");
    let engine_err: EngineError = io_err.into();

    match engine_err {
        EngineError::Io(msg) => {
            assert!(msg.contains("access denied"));
        }
        _ => panic!("Expected EngineError::Io, got {:?}", engine_err),
    }
}

#[test]
fn test_serde_json_error_conversion() {
    let invalid_json = "{ \"bad\": json }";
    let result: Result<serde_json::Value, serde_json::Error> = serde_json::from_str(invalid_json);
    
    let serde_err = result.expect_err("Parsing invalid JSON should yield an error");
    let engine_err: EngineError = serde_err.into();

    assert!(matches!(engine_err, EngineError::Parsing(_)));
}

#[test]
fn test_reqwest_error_conversion() {
    // Forcibly generate a `reqwest::Error` by passing a structurally malformed URL to the builder
    let reqwest_err = reqwest::Client::new()
        .get("ht tp://invalid-url-schema")
        .build()
        .expect_err("Building request with an invalid URL schema must fail");

    let engine_err: EngineError = reqwest_err.into();

    assert!(matches!(engine_err, EngineError::Network(_)));
}