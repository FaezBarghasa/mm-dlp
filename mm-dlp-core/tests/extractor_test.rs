use uniffi_mmdlp::extractor::registry::route_url;
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
    let client = Client::new();
    let res = client
        .get("https://open.spotify.com/embed/track/4PTG3Z6ehGkBF3zI7Ywt8D")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .expect("Failed to send request");

    let body = res.text().await.expect("Failed to get text");
    println!("Body length: {}", body.len());
    
    // Find script tags
    for line in body.lines() {
        if line.contains("<script") && (line.contains("json") || line.contains("resource") || line.contains("Spotify")) {
            println!("Script line: {}", &line[..200.min(line.len())]);
        }
    }
}