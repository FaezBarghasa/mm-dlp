use crate::domain::playlist::{Playlist, Track};
use crate::extractor::traits::AudioSource;
use crate::data::playlist::{json_handler, xml_handler};

fn create_test_playlist() -> Playlist {
    Playlist {
        id: "pl-123".to_string(),
        name: "Test Playlist".to_string(),
        description: Some("A playlist for testing".to_string()),
        tracks: vec![
            Track {
                id: "track-1".to_string(),
                title: "Track 1".to_string(),
                artist: "Artist 1".to_string(),
                album: Some("Album 1".to_string()),
                source_url: "http://example.com/track1".to_string(),
                duration: 180,
            },
            Track {
                id: "track-2".to_string(),
                title: "Track 2".to_string(),
                artist: "Artist 2".to_string(),
                album: None,
                source_url: "http://example.com/track2".to_string(),
                duration: 240,
            },
        ],
        source: AudioSource::YouTubeMusic,
    }
}

#[test]
fn test_json_round_trip() {
    let playlist = create_test_playlist();
    let json = json_handler::export_to_json(&playlist).unwrap();
    let imported_playlist = json_handler::import_from_json(&json).unwrap();
    assert_eq!(playlist, imported_playlist);
}

#[test]
fn test_xml_round_trip() {
    let playlist = create_test_playlist();
    let xml = xml_handler::export_to_xml(&playlist).unwrap();
    let imported_playlist = xml_handler::import_from_xml(&xml).unwrap();
    assert_eq!(playlist, imported_playlist);
}

#[test]
fn test_malformed_json() {
    let malformed_json = r#"{"id": "pl-123", "name": "Test", "tracks": [{"title": "Track 1"}]}"#;
    let result = json_handler::import_from_json(malformed_json);
    assert!(result.is_err());
}

#[test]
fn test_malformed_xml() {
    let malformed_xml = r#"<Playlist><id>pl-123</id><name>Test</name><track><title>Track 1</title></track></Playlist>"#;
    let result = xml_handler::import_from_xml(malformed_xml);
    assert!(result.is_err());
}
