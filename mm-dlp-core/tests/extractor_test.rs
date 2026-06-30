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
    let video_id = "dQw4w9WgXcQ";
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20240624.01.00",
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id
    });

    let res = client
        .post("https://music.youtube.com/youtubei/v1/player")
        .header("Content-Type", "application/json")
        .header("User-Agent", "com.google.android.apps.youtube.music/7.27.52 (Linux; U; Android 11) gzip")
        .header("X-Goog-Api-Format-Version", "1")
        .header("X-YouTube-Client-Name", "21")
        .header("X-YouTube-Client-Version", "7.27.52")
        .header("x-goog-api-key", "AIzaSyAOghZGza2MQSZkY_zfZ370N-PUdXEo8AI")
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    let json: serde_json::Value = res.json().await.expect("Failed to parse JSON");
    println!("Response keys: {:?}", json.as_object().map(|m| m.keys().collect::<Vec<_>>()));
    
    if let Some(playability) = json.get("playabilityStatus") {
        println!("PlayabilityStatus: {}", serde_json::to_string_pretty(playability).unwrap());
    }
    
    if let Some(streaming_data) = json.get("streamingData") {
        println!("streamingData exists! Keys: {:?}", streaming_data.as_object().map(|m| m.keys().collect::<Vec<_>>()));
    } else {
        println!("NO streamingData found in the root response!");
    }
}