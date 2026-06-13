use mm_dlp_core::extractor::registry::route_url;
use reqwest::Client;

#[test]
fn test_route_url_youtube_valid() {
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let extractor = route_url(url).expect("Expected valid match for YouTubeExtractor");
    
    // Evaluated directly
    assert!(extractor.matches_url(url));
}

#[test]
fn test_route_url_vimeo_valid() {
    let url = "https://vimeo.com/123456789";
    let extractor = route_url(url).expect("Expected valid match for VimeoExtractor");
    
    assert!(extractor.matches_url(url));
}

#[test]
fn test_route_url_invalid_unknown() {
    let url = "https://www.example.com/video/123";
    let extractor = route_url(url);
    
    assert!(extractor.is_none(), "Unknown URLs should not map to any extractor");
}

#[tokio::test]
async fn test_async_extract_metadata_youtube() {
    let url = "https://youtu.be/dQw4w9WgXcQ";
    let extractor = route_url(url).expect("Expected valid match for YouTubeExtractor");
    
    let client = Client::new();
    
    // Executed offline safely since `YouTubeExtractor` doesn't strictly execute a web request in our baseline implementation.
    let metadata = extractor.extract_metadata(&client, url).await.expect("Failed to extract metadata");
    
    assert_eq!(metadata.id, "dQw4w9WgXcQ");
    assert_eq!(metadata.title, "YouTube Video dQw4w9WgXcQ");
    assert_eq!(metadata.duration, Some(240));
    
    let best_format = metadata.formats.first().expect("Formats shouldn't be empty");
    assert_eq!(best_format.format_id, "137");
    assert_eq!(best_format.ext, "mp4");
    assert_eq!(best_format.width, Some(1920));
    assert_eq!(best_format.height, Some(1080));
}