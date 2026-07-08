use crate::ffi::types::{AudioSource, AudioQuality, Playlist, Track};
use crate::extractor::traits::{AudioSource as DomainAudioSource, AudioQuality as DomainAudioQuality};
use crate::domain::playlist::{Playlist as DomainPlaylist, Track as DomainTrack};

#[test]
fn test_type_conversions() {
    // AudioSource
    let domain_source = DomainAudioSource::YouTubeMusic;
    let ffi_source: AudioSource = domain_source.into();
    assert_eq!(ffi_source, AudioSource::YouTubeMusic);
    let converted_back: DomainAudioSource = ffi_source.into();
    assert_eq!(converted_back, domain_source);

    // AudioQuality
    let domain_quality = DomainAudioQuality::High;
    let ffi_quality: AudioQuality = domain_quality.into();
    assert_eq!(ffi_quality, AudioQuality::High);
    let converted_back_quality: DomainAudioQuality = ffi_quality.into();
    assert_eq!(converted_back_quality, domain_quality);

    // Playlist
    let domain_playlist = DomainPlaylist {
        id: "pl-1".to_string(),
        name: "Test".to_string(),
        description: None,
        tracks: vec![DomainTrack {
            id: "track-1".to_string(),
            title: "Title".to_string(),
            artist: "Artist".to_string(),
            album: None,
            source_url: "url".to_string(),
            duration: 180,
        }],
        source: DomainAudioSource::SoundCloud,
    };
    let ffi_playlist: Playlist = domain_playlist.clone().into();
    let converted_back_playlist: DomainPlaylist = ffi_playlist.into();
    assert_eq!(domain_playlist, converted_back_playlist);
}
