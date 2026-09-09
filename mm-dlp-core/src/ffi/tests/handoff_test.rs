use crate::ffi::api::MmDlpApi;
use crate::ffi::types::{AudioSource, AudioQuality};
use std::path::Path;
use tokio::fs;

#[tokio::test]
#[ignore] // This test requires a network connection and a valid track URL
async fn test_file_handoff() {
    let api = MmDlpApi::new().unwrap();
    let temp_dir = std::env::temp_dir();
    let temp_dir_str = temp_dir.to_str().unwrap().to_string();

    // A known track URL for testing
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string();

    let result = api.download_track(url, AudioQuality::Medium, None, temp_dir_str);
    assert!(result.is_ok());

    let file_path_str = result.unwrap();
    let file_path = Path::new(&file_path_str);
    assert!(file_path.exists());

    // Simulate Kotlin moving the file
    let new_path = temp_dir.join("moved_file");
    fs::rename(file_path, &new_path).await.unwrap();
    assert!(!file_path.exists());
    assert!(new_path.exists());

    // Cleanup
    fs::remove_file(new_path).await.unwrap();
}
