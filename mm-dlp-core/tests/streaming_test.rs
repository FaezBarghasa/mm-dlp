use mm_dlp_core::data::router::StreamRouter;
use mm_dlp_core::domain::models::{AudioSource, AudioQuality};

#[tokio::test]
#[ignore]
async fn test_youtube_music_stream() {
    let router = StreamRouter::new().await.unwrap();
    // A known track ID for testing, e.g., "dQw4w9WgXcQ"
    let result = router.get_stream(&AudioSource::YouTubeMusic, "dQw4w9WgXcQ", AudioQuality::Medium).await;
    assert!(result.is_ok());
    let stream_info = result.unwrap();
    assert!(!stream_info.stream_url.is_empty());
    assert_eq!(stream_info.metadata.title, "Rick Astley - Never Gonna Give You Up (Official Music Video)");
}

#[tokio::test]
#[ignore]
async fn test_soundcloud_stream() {
    let router = StreamRouter::new().await.unwrap();
    // A known track ID for testing, e.g., "85441169" (Mura Masa - Lotus Eater)
    let result = router.get_stream(&AudioSource::SoundCloud, "85441169", AudioQuality::Medium).await;
    assert!(result.is_ok());
    let stream_info = result.unwrap();
    assert!(!stream_info.stream_url.is_empty());
    assert_eq!(stream_info.metadata.title, "Lotus Eater");
}
