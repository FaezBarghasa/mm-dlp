use crate::domain::playlist::Playlist;
use crate::data::playlist::json_handler::PlaylistError;

/// Serializes a `Playlist` to an indented XML string using `quick-xml`'s serde integration.
pub fn export_to_xml(playlist: &Playlist) -> Result<String, PlaylistError> {
    // quick_xml::se::to_string is the correct entry point for serde serialization
    let mut buf = String::new();
    let mut serializer = quick_xml::se::Serializer::new(&mut buf);
    serializer.indent(' ', 2);
    serde::Serialize::serialize(playlist, serializer)
        .map_err(|e| PlaylistError::Serialization(e.to_string()))?;
    Ok(buf)
}

/// Deserializes a `Playlist` from an XML string and validates required fields.
pub fn import_from_xml(xml_str: &str) -> Result<Playlist, PlaylistError> {
    // quick_xml::de::from_str is the correct entry point for serde deserialization
    let playlist: Playlist = quick_xml::de::from_str(xml_str)
        .map_err(|e| PlaylistError::Deserialization(e.to_string()))?;
    validate_playlist(&playlist)?;
    Ok(playlist)
}

fn validate_playlist(playlist: &Playlist) -> Result<(), PlaylistError> {
    if playlist.name.is_empty() {
        return Err(PlaylistError::InvalidFormat(
            "Playlist name must not be empty".to_string(),
        ));
    }
    for track in &playlist.tracks {
        if track.title.is_empty() || track.artist.is_empty() || track.source_url.is_empty() {
            return Err(PlaylistError::InvalidFormat(format!(
                "Track '{}' is missing required fields (title, artist, or source_url)",
                track.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::playlist::{Playlist, Track};
    use crate::extractor::traits::AudioSource;

    fn sample_playlist() -> Playlist {
        Playlist {
            id: "pl-001".to_string(),
            name: "Test Playlist".to_string(),
            description: Some("A test".to_string()),
            tracks: vec![Track {
                id: "t-001".to_string(),
                title: "Test Track".to_string(),
                artist: "Test Artist".to_string(),
                album: Some("Test Album".to_string()),
                source_url: "https://example.com/track/1".to_string(),
                duration: 180,
            }],
            source: AudioSource::YouTubeMusic,
        }
    }

    #[test]
    fn test_xml_roundtrip() {
        let original = sample_playlist();
        let xml = export_to_xml(&original).expect("export failed");
        assert!(xml.contains("Test Playlist"));
        let imported = import_from_xml(&xml).expect("import failed");
        assert_eq!(imported.name, original.name);
        assert_eq!(imported.tracks.len(), 1);
        assert_eq!(imported.tracks[0].title, original.tracks[0].title);
    }

    #[test]
    fn test_xml_validation_rejects_empty_title() {
        let mut playlist = sample_playlist();
        playlist.tracks[0].title = String::new();
        let xml = export_to_xml(&playlist).expect("export ok");
        let result = import_from_xml(&xml);
        assert!(result.is_err());
    }
}
