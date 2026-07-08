//! Round-trip serialization tests for JSON and XML playlist handlers.

use uniffi_mmdlp::domain::playlist::{Playlist, Track};
use uniffi_mmdlp::data::playlist::{json_handler, xml_handler};
use uniffi_mmdlp::extractor::traits::AudioSource;

fn make_playlist(track_count: usize) -> Playlist {
    Playlist {
        id: "pl-test-001".to_string(),
        name: "Round-trip Test Playlist".to_string(),
        description: Some("Unit test playlist".to_string()),
        tracks: (0..track_count)
            .map(|i| Track {
                id: format!("track-{:04}", i),
                title: format!("Track Number {}", i),
                artist: format!("Artist {}", i % 10),
                album: Some(format!("Album {}", i / 10)),
                source_url: format!("https://example.com/track/{}", i),
                duration: 180 + i as u64,
            })
            .collect(),
        source: AudioSource::SoundCloud,
    }
}

// ─── JSON Round-trip ─────────────────────────────────────────────────────────

#[test]
fn test_json_export_produces_valid_json() {
    let playlist = make_playlist(5);
    let json = json_handler::export_to_json(&playlist).expect("JSON export failed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON produced");
    assert_eq!(parsed["name"].as_str().unwrap(), "Round-trip Test Playlist");
    assert_eq!(parsed["tracks"].as_array().unwrap().len(), 5);
}

#[test]
fn test_json_roundtrip_identity() {
    let original = make_playlist(10);
    let json = json_handler::export_to_json(&original).expect("JSON export failed");
    let imported = json_handler::import_from_json(&json).expect("JSON import failed");
    assert_eq!(imported.id, original.id);
    assert_eq!(imported.name, original.name);
    assert_eq!(imported.tracks.len(), original.tracks.len());
    assert_eq!(imported.tracks[5].title, original.tracks[5].title);
    assert_eq!(imported.tracks[5].artist, original.tracks[5].artist);
}

#[test]
fn test_json_import_rejects_missing_title() {
    let json = r#"{
        "id": "bad-pl",
        "name": "Bad Playlist",
        "tracks": [
            {"id":"t1","title":"","artist":"Artist","album":null,"source_url":"https://ex.com","duration":100}
        ],
        "source": "SoundCloud"
    }"#;
    let result = json_handler::import_from_json(json);
    assert!(result.is_err(), "Expected error for empty title");
}

#[test]
fn test_json_import_rejects_missing_source_url() {
    let json = r#"{
        "id": "bad-pl",
        "name": "Bad Playlist",
        "tracks": [
            {"id":"t1","title":"Track","artist":"Artist","album":null,"source_url":"","duration":100}
        ],
        "source": "YouTubeMusic"
    }"#;
    let result = json_handler::import_from_json(json);
    assert!(result.is_err(), "Expected error for empty source_url");
}

// ─── XML Round-trip ──────────────────────────────────────────────────────────

#[test]
fn test_xml_export_produces_valid_xml() {
    let playlist = make_playlist(3);
    let xml = xml_handler::export_to_xml(&playlist).expect("XML export failed");
    assert!(xml.contains("Round-trip Test Playlist"), "Playlist name missing from XML");
    assert!(xml.contains("Track Number 0"), "First track title missing from XML");
}

#[test]
fn test_xml_roundtrip_identity() {
    let original = make_playlist(5);
    let xml = xml_handler::export_to_xml(&original).expect("XML export failed");
    let imported = xml_handler::import_from_xml(&xml).expect("XML import failed");
    assert_eq!(imported.id, original.id);
    assert_eq!(imported.name, original.name);
    assert_eq!(imported.tracks.len(), original.tracks.len());
    assert_eq!(imported.tracks[2].title, original.tracks[2].title);
}

// ─── Large Playlist ──────────────────────────────────────────────────────────

#[test]
fn test_json_large_playlist_roundtrip() {
    let original = make_playlist(10_000);
    let json = json_handler::export_to_json(&original).expect("JSON export of 10k tracks failed");
    let imported = json_handler::import_from_json(&json).expect("JSON import of 10k tracks failed");
    assert_eq!(imported.tracks.len(), 10_000);
    assert_eq!(imported.tracks[9999].title, "Track Number 9999");
}
