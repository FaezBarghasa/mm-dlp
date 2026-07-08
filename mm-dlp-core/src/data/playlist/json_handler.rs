use crate::domain::playlist::Playlist;
use anyhow::Result;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PlaylistError {
    #[error("Invalid playlist format: {0}")]
    InvalidFormat(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
}

pub fn export_to_json(playlist: &Playlist) -> Result<String, PlaylistError> {
    serde_json::to_string_pretty(playlist)
        .map_err(|e| PlaylistError::Serialization(e.to_string()))
}

pub fn import_from_json(json_str: &str) -> Result<Playlist, PlaylistError> {
    let playlist: Playlist = serde_json::from_str(json_str)
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
