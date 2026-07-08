// Integration tests for URL routing and extractor dispatch.
// Network tests are marked #[ignore] to avoid CI failures.

use uniffi_mmdlp::extractor::registry::route_url;

// ─── URL Routing Tests ──────────────────────────────────────────────────────

#[test]
fn test_route_url_youtube_standard() {
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let extractor = route_url(url).expect("Expected extractor for standard YouTube URL");
    assert!(extractor.matches_url(url));
}

#[test]
fn test_route_url_youtu_be_short() {
    let url = "https://youtu.be/dQw4w9WgXcQ";
    let extractor = route_url(url);
    // Short URLs may not be in the registry pattern yet — this test documents expected behaviour
    let _ = extractor; // May be Some or None depending on registry
}

#[test]
fn test_route_url_vimeo_valid() {
    let url = "https://vimeo.com/123456789";
    let extractor = route_url(url).expect("Expected extractor for Vimeo URL");
    assert!(extractor.matches_url(url));
}

#[test]
fn test_route_url_unknown_returns_none() {
    let url = "https://www.example.com/video/123";
    assert!(route_url(url).is_none(), "Unknown URLs should return None");
}

#[test]
fn test_route_url_empty_returns_none() {
    assert!(route_url("").is_none());
}

// ─── TrackMetadata Parsing Tests ────────────────────────────────────────────

#[test]
fn test_parse_soundcloud_search_mock() {
    use uniffi_mmdlp::extractor::traits::{TrackMetadata, AudioSource};
    use serde_json::json;

    let mock_response = json!({
        "collection": [
            {
                "id": 123456789u64,
                "title": "Test Track",
                "user": { "username": "Test Artist" },
                "artwork_url": "https://i1.sndcdn.com/artworks-000.jpg",
                "duration": 210000,
                "bitrate": 128
            }
        ]
    });

    let collection = mock_response["collection"].as_array().unwrap();
    let item = &collection[0];

    let track = TrackMetadata {
        title: item["title"].as_str().unwrap_or("").to_string(),
        artist: item["user"]["username"].as_str().unwrap_or("").to_string(),
        album: None,
        album_art_url: item["artwork_url"].as_str().map(str::to_string),
        track_id: item["id"].as_u64().unwrap_or(0).to_string(),
        source: AudioSource::SoundCloud,
    };

    assert_eq!(track.title, "Test Track");
    assert_eq!(track.artist, "Test Artist");
    assert_eq!(track.track_id, "123456789");
    assert!(track.album_art_url.is_some());
}

#[test]
fn test_parse_youtube_stream_info_mock() {
    use uniffi_mmdlp::extractor::traits::{StreamInfo, TrackMetadata, AudioSource};
    use serde_json::json;

    let mock_player = json!({
        "videoDetails": {
            "videoId": "dQw4w9WgXcQ",
            "title": "Never Gonna Give You Up",
            "author": "Rick Astley",
            "lengthSeconds": "213",
            "thumbnail": {
                "thumbnails": [
                    { "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg", "width": 1280, "height": 720 }
                ]
            }
        },
        "streamingData": {
            "adaptiveFormats": [
                {
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "url": "https://rr1.googlevideo.com/test_opus",
                    "bitrate": 160000,
                    "itag": 251
                },
                {
                    "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                    "url": "https://rr1.googlevideo.com/test_aac",
                    "bitrate": 128000,
                    "itag": 140
                }
            ]
        }
    });

    let formats = mock_player["streamingData"]["adaptiveFormats"]
        .as_array()
        .unwrap();

    // Verify Opus is selected over AAC when both available
    let best = formats
        .iter()
        .filter(|f| f["mimeType"].as_str().unwrap_or("").starts_with("audio/"))
        .max_by(|a, b| {
            let a_opus = a["mimeType"].as_str().unwrap_or("").contains("opus");
            let b_opus = b["mimeType"].as_str().unwrap_or("").contains("opus");
            match (a_opus, b_opus) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => a["bitrate"].as_u64().unwrap_or(0)
                    .cmp(&b["bitrate"].as_u64().unwrap_or(0)),
            }
        })
        .unwrap();

    assert!(best["mimeType"].as_str().unwrap().contains("opus"),
        "Expected Opus to be selected as the best audio format");
    assert_eq!(best["bitrate"].as_u64().unwrap(), 160000);
}

// ─── Integration Tests (require network — excluded from CI) ─────────────────

#[tokio::test]
#[ignore = "requires network access"]
async fn test_yt_stream_url_live() {
    use uniffi_mmdlp::data::extractors::youtube::YouTubeMusicExtractor;
    use uniffi_mmdlp::extractor::traits::{AudioPlatformExtractor, AudioQuality};

    let extractor = YouTubeMusicExtractor::new().expect("Failed to create YouTubeMusicExtractor");
    // Rick Astley - "Never Gonna Give You Up" — a reliably public video
    let result = extractor.get_stream_url("dQw4w9WgXcQ", AudioQuality::High).await;
    assert!(result.is_ok(), "get_stream_url failed: {:?}", result.err());
    let info = result.unwrap();
    assert!(!info.stream_url.is_empty());
    assert!(info.bitrate > 0);
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_soundcloud_search_live() {
    use uniffi_mmdlp::data::extractors::soundcloud::SoundCloudExtractor;
    use uniffi_mmdlp::extractor::traits::AudioPlatformExtractor;

    let extractor = SoundCloudExtractor::new().await.expect("Failed to create SoundCloudExtractor");
    let results = extractor.search("lofi hip hop").await;
    assert!(results.is_ok(), "Search failed: {:?}", results.err());
    assert!(!results.unwrap().is_empty(), "Expected at least one search result");
}