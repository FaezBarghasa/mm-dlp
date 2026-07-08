use std::path::Path;
use anyhow::{anyhow, Result};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::picture::{MimeType, Picture, PictureType};
use crate::extractor::traits::TrackMetadata;

/// Tags the audio file at `file_path` with the provided `metadata` and optionally embeds
/// cover art downloaded from `cover_art_url`.
///
/// Uses `lofty` which supports ID3v2 (MP3), MP4 atoms (M4A/AAC), Vorbis comments (FLAC/OGG),
/// and Opus tags — all automatically detected from the file.
pub async fn tag_audio_file(
    file_path: &Path,
    metadata: &TrackMetadata,
    cover_art_url: &Option<String>,
) -> Result<()> {
    // Optionally fetch cover art bytes before touching the file
    let cover_art_bytes: Option<Vec<u8>> = if let Some(url) = cover_art_url {
        let response = reqwest::get(url)
            .await
            .map_err(|e| anyhow!("Failed to fetch cover art from {}: {}", url, e))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow!("Failed to read cover art bytes: {}", e))?;
        Some(bytes.to_vec())
    } else {
        None
    };

    // File I/O is synchronous in lofty; run it on a blocking thread to avoid
    // blocking the async executor.
    let file_path = file_path.to_path_buf();
    let metadata = metadata.clone();

    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut tagged_file = Probe::open(&file_path)
            .map_err(|e| anyhow!("lofty: failed to open file {:?}: {}", file_path, e))?
            .read()
            .map_err(|e| anyhow!("lofty: failed to read tags from {:?}: {}", file_path, e))?;

        // lofty resolves the best tag format for each container automatically
        let tag = tagged_file
            .primary_tag_mut()
            .ok_or_else(|| anyhow!("lofty: no primary tag found in {:?}", file_path))?;

        tag.set_title(metadata.title.clone());
        tag.set_artist(metadata.artist.clone());

        if let Some(album) = &metadata.album {
            tag.set_album(album.clone());
        }

        if let Some(art_bytes) = cover_art_bytes {
            // Detect MIME type from magic bytes (JPEG starts with FF D8; PNG starts with 89 50)
            let mime = if art_bytes.starts_with(&[0xFF, 0xD8]) {
                MimeType::Jpeg
            } else if art_bytes.starts_with(&[0x89, 0x50]) {
                MimeType::Png
            } else {
                MimeType::Jpeg // Default assumption
            };
            let picture = Picture::unchecked(art_bytes)
                .pic_type(PictureType::CoverFront)
                .mime_type(mime)
                .build();
            tag.push_picture(picture);
        }

        // lofty 0.22+ API: save back to the original path via the tagged file handle
        tagged_file
            .save_to_path(&file_path, lofty::config::WriteOptions::default())
            .map_err(|e| anyhow!("lofty: failed to save tags to {:?}: {}", file_path, e))?;

        Ok(())
    })
    .await
    .map_err(|e| anyhow!("spawn_blocking panicked: {}", e))??;

    Ok(())
}
