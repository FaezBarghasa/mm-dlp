use serde::{Deserialize, Serialize};
use crate::extractor::traits::AudioSource;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(alias = "track", default)]
    pub tracks: Vec<Track>,
    pub source: AudioSource,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub source_url: String,
    pub duration: u64,
}
