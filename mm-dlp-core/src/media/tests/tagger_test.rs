use crate::media::tagger::tag_audio_file;
use crate::extractor::traits::{TrackMetadata, AudioSource};
use lofty::prelude::*;
use lofty::probe::Probe;
use std::path::Path;

#[tokio::test]
async fn test_tagging() {
    let dummy_file = Path::new("dummy.wav");
    let metadata = TrackMetadata {
        title: "Test Title".to_string(),
        artist: "Test Artist".to_string(),
        album: Some("Test Album".to_string()),
        album_art_url: None,
        track_id: "123".to_string(),
        source: AudioSource::YouTubeMusic,
    };

    // Create a copy for tagging
    let test_file = Path::new("test_dummy.wav");
    tokio::fs::copy(dummy_file, test_file).await.unwrap();

    tag_audio_file(test_file, &metadata, &None).await.unwrap();

    let tagged_file = Probe::open(test_file).unwrap().read(true).unwrap();
    let tag = tagged_file.primary_tag().unwrap();

    assert_eq!(tag.title(), Some("Test Title"));
    assert_eq!(tag.artist(), Some("Test Artist"));
    assert_eq!(tag.album(), Some("Test Album"));

    tokio::fs::remove_file(test_file).await.unwrap();
}
