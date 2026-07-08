use std::path::{Path, PathBuf};
use anyhow::Result;
use tokio::sync::mpsc;
use crate::media::converter::{self, AudioFormat};
use crate::media::tagger;
use crate::extractor::traits::TrackMetadata;

struct TempFileGuard {
    path: PathBuf,
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub async fn process_downloaded_file(
    input_path: &Path,
    metadata: &TrackMetadata,
    cover_art_url: &Option<String>,
    target_format: Option<AudioFormat>,
    progress_sender: mpsc::Sender<String>,
) -> Result<()> {
    let input_guard = TempFileGuard { path: input_path.to_path_buf() };

    let final_path = if let Some(format) = target_format {
        let output_path = input_path.with_extension(match format {
            AudioFormat::Flac => "flac",
            AudioFormat::Wav => "wav",
            AudioFormat::Mp3 => "mp3",
        });
        converter::convert_format(input_path, &output_path, format, progress_sender).await?;
        output_path
    } else {
        input_path.to_path_buf()
    };

    tagger::tag_audio_file(&final_path, metadata, cover_art_url).await?;

    // Disarm the guard since processing is complete
    std::mem::forget(input_guard);

    Ok(())
}
