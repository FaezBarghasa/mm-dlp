use std::path::Path;
use anyhow::{anyhow, Result};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType, ItemValue, Picture};
use reqwest;
use crate::extractor::traits::TrackMetadata;

pub async fn tag_audio_file(file_path: &Path, metadata: &TrackMetadata, cover_art_url: &Option<String>) -> Result<()> {
    let mut tagged_file = Probe::open(file_path)?.read(true)?;

    let tag_type = tagged_file.primary_tag_type().unwrap_or(TagType::Id3v2);
    let tag = tagged_file.primary_tag_mut().get_or_insert_with(|| Tag::new(tag_type));

    tag.set_title(metadata.title.clone());
    tag.set_artist(metadata.artist.clone());
    if let Some(album) = &metadata.album {
        tag.set_album(album.clone());
    }

    if let Some(url) = cover_art_url {
        let response = reqwest::get(url).await?;
        let cover_art_data = response.bytes().await?;
        let picture = Picture::new_unchecked(MimeType::Jpeg, Some(PictureType::CoverFront), None, cover_art_data.to_vec());
        tag.push_picture(picture);
    }

    tag.save_to(file_path)?;

    Ok(())
}
