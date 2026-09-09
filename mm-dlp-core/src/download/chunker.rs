use crate::network::quic_client::QuicHttpClient;
use anyhow::{anyhow, Result};
use std::path::Path;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::time::{sleep, Duration};
use url::Url;

/// The size of each download chunk (4 MiB).
pub(crate) const CHUNK_SIZE: u64 = 1024 * 1024 * 4;

/// Downloads a specific byte range from `url`, retrying up to 3 times with exponential backoff.
/// Returns the raw bytes for the requested range.
pub async fn download_chunk(
    client: &QuicHttpClient,
    url: &Url,
    start: u64,
    end: u64,
) -> Result<bytes::Bytes> {
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..3u32 {
        match client.get_range(url, start, end).await {
            Ok(response) => {
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|e| anyhow!("Failed to read chunk bytes: {}", e))?;
                return Ok(bytes);
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < 2 {
                    sleep(Duration::from_secs(2u64.pow(attempt))).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Chunk download failed after 3 retries")))
}

/// Assembles all downloaded chunk files in the `.part` directory into the final file,
/// then removes the chunk files and the `.part` directory.
pub async fn assemble_chunks(destination_path: &Path, _total_size: u64) -> Result<()> {
    let part_path = destination_path.with_extension("part");
    let mut final_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(destination_path)
        .await?;

    let mut dir = fs::read_dir(&part_path).await?;
    let mut chunks = Vec::new();
    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with("chunk_") {
                    if let Ok(index) = file_name["chunk_".len()..].parse::<usize>() {
                        chunks.push((index, path));
                    }
                }
            }
        }
    }

    if chunks.is_empty() {
        return Err(anyhow!("No chunks found in part directory"));
    }

    chunks.sort_by_key(|(index, _)| *index);

    for (i, (index, _)) in chunks.iter().enumerate() {
        if *index != i {
            return Err(anyhow!("Missing chunk index: expected chunk_{}, but got chunk_{}", i, index));
        }
    }

    for (_, chunk_path) in &chunks {
        let mut chunk_file = File::open(chunk_path).await?;
        let mut buffer = Vec::new();
        chunk_file.read_to_end(&mut buffer).await?;
        final_file.write_all(&buffer).await?;
        fs::remove_file(chunk_path).await?;
    }

    final_file.flush().await?;
    drop(final_file);

    fs::remove_dir_all(&part_path).await?;
    Ok(())
}

/// Calculates byte range pairs for a file of `total_size` bytes, split into `CHUNK_SIZE` chunks.
pub fn calculate_chunks(total_size: u64) -> Vec<(u64, u64)> {
    if total_size == 0 {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0u64;
    while start < total_size {
        let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
        chunks.push((start, end));
        start += CHUNK_SIZE;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_chunks_empty() {
        assert!(calculate_chunks(0).is_empty());
    }

    #[test]
    fn test_calculate_chunks_single() {
        let chunks = calculate_chunks(1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], (0, 999));
    }

    #[test]
    fn test_calculate_chunks_exact_multiple() {
        let size = CHUNK_SIZE * 4;
        let chunks = calculate_chunks(size);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0], (0, CHUNK_SIZE - 1));
        assert_eq!(chunks[1], (CHUNK_SIZE, CHUNK_SIZE * 2 - 1));
        assert_eq!(chunks[3].1, size - 1);
    }

    #[test]
    fn test_calculate_chunks_partial_last() {
        // total size is 4MB + 500 bytes → 2 chunks
        let size = CHUNK_SIZE + 500;
        let chunks = calculate_chunks(size);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], (0, CHUNK_SIZE - 1));
        assert_eq!(chunks[1], (CHUNK_SIZE, size - 1));
    }
}
