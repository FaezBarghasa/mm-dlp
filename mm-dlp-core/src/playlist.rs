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
    serde_json::to_string_pretty(playlist).map_err(|e| EngineError::Serialization(e.to_string()))
}

pub fn import_playlist_from_json(json_str: &str) -> Result<Playlist, EngineError> {
    let playlist: Playlist =
        serde_json::from_str(json_str).map_err(|e| EngineError::Serialization(e.to_string()))?;

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
    quick_xml::se::to_string(playlist).map_err(|e| EngineError::Serialization(e.to_string()))
}

pub fn import_playlist_from_xml(xml_str: &str) -> Result<Playlist, EngineError> {
    let playlist: Playlist =
        quick_xml::de::from_str(xml_str).map_err(|e| EngineError::Serialization(e.to_string()))?;

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

/// Slices playlist tracks according to 1-based index specification or start/end bounds.
/// Supports comma-separated indices and ranges: e.g. "1,2,5-8", "1:5", "3".
pub fn slice_playlist_items(
    tracks: &[PlaylistTrack],
    items_spec: Option<&str>,
    start: Option<usize>,
    end: Option<usize>,
) -> Vec<PlaylistTrack> {
    if tracks.is_empty() {
        return Vec::new();
    }

    if let Some(spec) = items_spec
        && !spec.trim().is_empty()
    {
        let mut selected_indices = std::collections::BTreeSet::new();
        for part in spec.split(',') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            if let Some((s_str, e_str)) = p.split_once('-').or_else(|| p.split_once(':')) {
                let s: usize = s_str.trim().parse().unwrap_or(1);
                let e: usize = e_str.trim().parse().unwrap_or(tracks.len());
                let (min_idx, max_idx) = if s <= e { (s, e) } else { (e, s) };
                for idx in min_idx..=max_idx {
                    if idx >= 1 && idx <= tracks.len() {
                        selected_indices.insert(idx - 1);
                    }
                }
            } else if let Ok(idx) = p.parse::<usize>()
                && idx >= 1
                && idx <= tracks.len()
            {
                selected_indices.insert(idx - 1);
            }
        }

        return selected_indices
            .into_iter()
            .map(|i| tracks[i].clone())
            .collect();
    }

    let start_idx = start.unwrap_or(1).max(1) - 1;
    let end_idx = end.unwrap_or(tracks.len()).min(tracks.len());

    if start_idx >= end_idx || start_idx >= tracks.len() {
        return Vec::new();
    }

    tracks[start_idx..end_idx].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tracks() -> Vec<PlaylistTrack> {
        (1..=10)
            .map(|i| PlaylistTrack {
                title: format!("Track {}", i),
                artist: "Artist".into(),
                url: format!("https://example.com/{}", i),
            })
            .collect()
    }

    #[test]
    fn test_slice_playlist_items_spec_comma_and_range() {
        let tracks = sample_tracks();
        let sliced = slice_playlist_items(&tracks, Some("1,3,5-7"), None, None);
        assert_eq!(sliced.len(), 5);
        assert_eq!(sliced[0].title, "Track 1");
        assert_eq!(sliced[1].title, "Track 3");
        assert_eq!(sliced[2].title, "Track 5");
        assert_eq!(sliced[3].title, "Track 6");
        assert_eq!(sliced[4].title, "Track 7");
    }

    #[test]
    fn test_slice_playlist_items_colon_range() {
        let tracks = sample_tracks();
        let sliced = slice_playlist_items(&tracks, Some("2:4"), None, None);
        assert_eq!(sliced.len(), 3);
        assert_eq!(sliced[0].title, "Track 2");
        assert_eq!(sliced[1].title, "Track 3");
        assert_eq!(sliced[2].title, "Track 4");
    }

    #[test]
    fn test_slice_playlist_items_start_end() {
        let tracks = sample_tracks();
        let sliced = slice_playlist_items(&tracks, None, Some(3), Some(5));
        assert_eq!(sliced.len(), 3);
        assert_eq!(sliced[0].title, "Track 3");
        assert_eq!(sliced[1].title, "Track 4");
        assert_eq!(sliced[2].title, "Track 5");
    }
}
