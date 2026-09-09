use serde::{Deserialize, Serialize};

use crate::error::EngineError;

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct PlaylistTrack {
    pub title: String,
    pub artist: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct Playlist {
    pub version: String,
    pub name: String,
    pub tracks: Vec<PlaylistTrack>,
}

pub fn export_playlist_to_json(playlist: &Playlist) -> Result<String, EngineError> {
    serde_json::to_string_pretty(playlist)
        .map_err(|e| EngineError::Serialization(e.to_string()))
}

pub fn import_playlist_from_json(json_str: &str) -> Result<Playlist, EngineError> {
    let playlist: Playlist = serde_json::from_str(json_str)
        .map_err(|e| EngineError::Serialization(e.to_string()))?;

    if playlist.version != "1.0" {
        return Err(EngineError::InvalidConfig(format!(
            "Unsupported playlist version: {}",
            playlist.version
        )));
    }

    if playlist.tracks.is_empty() {
        return Err(EngineError::InvalidConfig(
            "Playlist track count cannot be zero".to_string(),
        ));
    }

    Ok(playlist)
}

pub fn export_playlist_to_xml(playlist: &Playlist) -> Result<String, EngineError> {
    quick_xml::se::to_string(playlist)
        .map_err(|e| EngineError::Serialization(e.to_string()))
}

pub fn import_playlist_from_xml(xml_str: &str) -> Result<Playlist, EngineError> {
    let playlist: Playlist = quick_xml::de::from_str(xml_str)
        .map_err(|e| EngineError::Serialization(e.to_string()))?;

    if playlist.version != "1.0" {
        return Err(EngineError::InvalidConfig(format!(
            "Unsupported playlist version: {}",
            playlist.version
        )));
    }

    if playlist.tracks.is_empty() {
        return Err(EngineError::InvalidConfig(
            "Playlist track count cannot be zero".to_string(),
        ));
    }

    Ok(playlist)
}
