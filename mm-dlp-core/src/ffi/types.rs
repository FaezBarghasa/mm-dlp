use crate::extractor::traits::{AudioSource as DomainAudioSource, AudioQuality as DomainAudioQuality};
use crate::domain::playlist::{Playlist as DomainPlaylist, Track as DomainTrack};
use uniffi::{Record, Enum};

#[derive(Debug, Enum, Clone, Copy)]
pub enum AudioSource {
    YouTubeMusic,
    SoundCloud,
    Spotify,
}

impl From<DomainAudioSource> for AudioSource {
    fn from(source: DomainAudioSource) -> Self {
        match source {
            DomainAudioSource::YouTubeMusic => Self::YouTubeMusic,
            DomainAudioSource::SoundCloud => Self::SoundCloud,
            DomainAudioSource::Spotify => Self::Spotify,
        }
    }
}

impl From<AudioSource> for DomainAudioSource {
    fn from(source: AudioSource) -> Self {
        match source {
            AudioSource::YouTubeMusic => Self::YouTubeMusic,
            AudioSource::SoundCloud => Self::SoundCloud,
            AudioSource::Spotify => Self::Spotify,
        }
    }
}

#[derive(Debug, Enum, Clone, Copy)]
pub enum AudioQuality {
    Low,
    Medium,
    High,
    Lossless,
}

impl From<DomainAudioQuality> for AudioQuality {
    fn from(quality: DomainAudioQuality) -> Self {
        match quality {
            DomainAudioQuality::Low => Self::Low,
            DomainAudioQuality::Medium => Self::Medium,
            DomainAudioQuality::High => Self::High,
            DomainAudioQuality::Lossless => Self::Lossless,
        }
    }
}

impl From<AudioQuality> for DomainAudioQuality {
    fn from(quality: AudioQuality) -> Self {
        match quality {
            AudioQuality::Low => Self::Low,
            AudioQuality::Medium => Self::Medium,
            AudioQuality::High => Self::High,
            AudioQuality::Lossless => Self::Lossless,
        }
    }
}

#[derive(Debug, Record, Clone)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub album_art_url: Option<String>,
    pub track_id: String,
    pub source: AudioSource,
}

#[derive(Debug, Record, Clone)]
pub struct StreamInfo {
    pub stream_url: String,
    pub format: String,
    pub bitrate: u32,
    pub duration_secs: u64,
    pub metadata: TrackMetadata,
}

#[derive(Debug, Record, Clone)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub source_url: String,
    pub duration: u64,
}

#[derive(Debug, Record, Clone)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tracks: Vec<Track>,
    pub source: AudioSource,
}

impl From<DomainPlaylist> for Playlist {
    fn from(playlist: DomainPlaylist) -> Self {
        Self {
            id: playlist.id,
            name: playlist.name,
            description: playlist.description,
            tracks: playlist.tracks.into_iter().map(Into::into).collect(),
            source: playlist.source.into(),
        }
    }
}

impl From<Playlist> for DomainPlaylist {
    fn from(playlist: Playlist) -> Self {
        Self {
            id: playlist.id,
            name: playlist.name,
            description: playlist.description,
            tracks: playlist.tracks.into_iter().map(Into::into).collect(),
            source: playlist.source.into(),
        }
    }
}

impl From<DomainTrack> for Track {
    fn from(track: DomainTrack) -> Self {
        Self {
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            source_url: track.source_url,
            duration: track.duration,
        }
    }
}

impl From<Track> for DomainTrack {
    fn from(track: Track) -> Self {
        Self {
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            source_url: track.source_url,
            duration: track.duration,
        }
    }
}
