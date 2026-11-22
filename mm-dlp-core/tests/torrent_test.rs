use mm_dlp_core::downloader::torrent::{
    TorrentDownloader, TorrentFileEntry, TorrentMetadata, parse_magnet_uri,
};
use mm_dlp_core::error::EngineError;
use std::path::PathBuf;

#[test]
fn test_parse_magnet_uri_standard() {
    let magnet = "magnet:?xt=urn:btih:cab507494d02ebb1178b38f2e9d7be299c86b862&dn=ubuntu-21.04-live-server-amd64.iso";
    let meta = parse_magnet_uri(magnet).expect("Valid magnet link should parse");
    assert_eq!(meta.info_hash, "cab507494d02ebb1178b38f2e9d7be299c86b862");
    assert_eq!(meta.name, "ubuntu-21.04-live-server-amd64.iso");
    assert_eq!(meta.total_bytes, 0);
    assert!(meta.files.is_empty());
}

#[test]
fn test_parse_magnet_uri_multi_trackers() {
    let magnet = "magnet:?xt=urn:btih:3b245504d5f72b1e7b638671948934f4f6920037&dn=Arch+Linux+2026&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce";
    let meta = parse_magnet_uri(magnet).expect("Magnet with multiple trackers should parse");
    assert_eq!(meta.info_hash, "3b245504d5f72b1e7b638671948934f4f6920037");
    assert_eq!(meta.name, "Arch Linux 2026");
}

#[test]
fn test_parse_magnet_invalid_input() {
    let invalid = "not_a_magnet_url";
    let err = parse_magnet_uri(invalid).unwrap_err();
    match err {
        EngineError::Torrent(msg) => {
            assert!(msg.contains("Invalid magnet URI"));
        }
        _ => panic!("Expected EngineError::Torrent, got {:?}", err),
    }
}

#[test]
fn test_torrent_metadata_roundtrip() {
    let files = vec![
        TorrentFileEntry {
            index: 0,
            path: "album/track1.flac".into(),
            length: 35000000,
        },
        TorrentFileEntry {
            index: 1,
            path: "album/track2.flac".into(),
            length: 42000000,
        },
    ];

    let meta = TorrentMetadata {
        name: "FLAC Album".into(),
        info_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
        total_bytes: 77000000,
        files,
    };

    let serialized = serde_json::to_string(&meta).expect("Serialization failed");
    let deserialized: TorrentMetadata =
        serde_json::from_str(&serialized).expect("Deserialization failed");

    assert_eq!(meta, deserialized);
    assert_eq!(deserialized.files.len(), 2);
    assert_eq!(deserialized.files[1].path, "album/track2.flac");
}

#[tokio::test]
async fn test_torrent_downloader_session_creation() {
    let temp_dir = std::env::temp_dir().join(format!("mm_dlp_test_{}", std::process::id()));
    let downloader = TorrentDownloader::new(temp_dir.clone()).expect("Should create session dir");

    // Invalid non-existent file should yield an EngineError::IoError or Torrent error
    let res = downloader.get_metadata("/non/existent/file.torrent").await;
    assert!(res.is_err());

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}
