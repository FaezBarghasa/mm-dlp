//! Playlist import and export module.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(uniffi::Record)]
pub struct PlaylistTrack {
    pub title: String,
    pub artist: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(uniffi::Record)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<PlaylistTrack>,
}
