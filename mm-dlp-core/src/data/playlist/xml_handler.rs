use crate::domain::playlist::Playlist;
use crate::data::playlist::json_handler::PlaylistError;
use anyhow::Result;
use quick_xml::se::{to_string, Serializer};
use quick_xml::de::{from_str, Deserializer};

pub fn export_to_xml(playlist: &Playlist) -> Result<String, PlaylistError> {
    let mut serializer = Serializer::new(Vec::new());
    serializer.indent(' ', 2);
    playlist.serialize(serializer)
        .map_err(|e| PlaylistError::Serialization(e.to_string()))?;
    String::from_utf8(serializer.into_inner())
        .map_err(|e| PlaylistError::Serialization(e.to_string()))
}

pub fn import_from_xml(xml_str: &str) -> Result<Playlist, PlaylistError> {
    let mut deserializer = Deserializer::from_str(xml_str);
    let playlist: Playlist = Playlist::deserialize(deserializer)
        .map_err(|e| PlaylistError::Deserialization(e.to_string()))?;
    validate_playlist(&playlist)?;
    Ok(playlist)
}

fn validate_playlist(playlist: &Playlist) -> Result<(), PlaylistError> {
    for track in &playlist.tracks {
        if track.title.is_empty() || track.artist.is_empty() || track.source_url.is_empty() {
            return Err(PlaylistError::InvalidFormat(format!("Track {} is missing required fields", track.id)));
        }
    }
    Ok(())
}
