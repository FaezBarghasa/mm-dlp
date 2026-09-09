use tokio::sync::{mpsc, watch};
use crate::network::quic_client::QuicHttpClient;
use crate::download::chunker::{self, calculate_chunks};
use anyhow::Result;
use std::path::{PathBuf};
use std::sync::Arc;
use thiserror::Error;
use url::Url;
use futures::stream::{self, StreamExt};

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("File system error: {0}")]
    FileSystem(String),
    #[error("Download failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed(DownloadError),
}

/// Describes a single download unit submitted to the manager.
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: Url,
    pub destination_path: PathBuf,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
}

/// Manages concurrent chunked downloads.
///
/// `DownloadManager::new()` creates the channel endpoints and returns immediately.
/// Call `spawn_worker()` inside a Tokio context to start the background loop.
pub struct DownloadManager {
    /// Arc-wrapped so it can be moved into the worker task.
    client: Arc<QuicHttpClient>,
    task_queue: mpsc::Sender<DownloadTask>,
    /// Broadcasts (url, downloaded_bytes, total_bytes).
    progress_broadcaster: watch::Sender<(String, u64, u64)>,
}

impl DownloadManager {
    /// Creates a `DownloadManager`. Call `spawn_worker()` after construction while
    /// inside a Tokio runtime to start the background processing loop.
    pub fn new() -> Result<(Self, mpsc::Receiver<DownloadTask>)> {
        let (task_tx, task_rx) = mpsc::channel(100);
        let (progress_tx, _) = watch::channel(("".to_string(), 0u64, 0u64));
        let client = Arc::new(QuicHttpClient::new()?);

        let manager = Self {
            client,
            task_queue: task_tx,
            progress_broadcaster: progress_tx,
        };

        Ok((manager, task_rx))
    }

    /// Spawns the background worker task. Must be called inside a Tokio runtime.
    pub fn spawn_worker(&self, mut task_rx: mpsc::Receiver<DownloadTask>) {
        let client = Arc::clone(&self.client);
        let progress_broadcaster = self.progress_broadcaster.clone();

        tokio::spawn(async move {
            while let Some(mut task) = task_rx.recv().await {
                task.status = DownloadStatus::Downloading;
                let _ = progress_broadcaster.send((
                    task.url.to_string(),
                    task.downloaded_bytes,
                    task.total_bytes,
                ));

                // Get total content length via a HEAD-equivalent GET (read content-length header)
                let total_size = match client.get(&task.url).await {
                    Ok(resp) => resp.content_length().unwrap_or(0),
                    Err(_) => 0,
                };
                task.total_bytes = total_size;

                let part_dir = task.destination_path.with_extension("part");
                if !part_dir.exists() {
                    if let Err(e) = tokio::fs::create_dir_all(&part_dir).await {
                        task.status =
                            DownloadStatus::Failed(DownloadError::FileSystem(e.to_string()));
                        let _ = progress_broadcaster.send((
                            task.url.to_string(),
                            0,
                            total_size,
                        ));
                        continue;
                    }
                }

                let chunks = calculate_chunks(total_size);
                let mut downloaded_bytes = 0u64;
                let mut had_error = false;

                let chunk_results: Vec<_> = stream::iter(chunks)
                    .map(|(start, end)| {
                        let client = Arc::clone(&client);
                        let url = task.url.clone();
                        let part_dir = part_dir.clone();
                        async move {
                            let chunk_index = start / chunker::CHUNK_SIZE;
                            let chunk_path =
                                part_dir.join(format!("chunk_{}", chunk_index));

                            // Skip already-downloaded chunks (resume support)
                            if chunk_path.exists() {
                                let size = tokio::fs::metadata(&chunk_path)
                                    .await
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                return Ok::<(u64, u64), anyhow::Error>((start, size));
                            }

                            let chunk = chunker::download_chunk(&client, &url, start, end)
                                .await?;
                            let size = chunk.len() as u64;
                            tokio::fs::write(&chunk_path, &chunk).await?;
                            Ok((start, size))
                        }
                    })
                    .buffer_unordered(4) // max 4 concurrent chunk downloads
                    .collect()
                    .await;

                for result in chunk_results {
                    match result {
                        Ok((_, size)) => {
                            downloaded_bytes += size;
                            let _ = progress_broadcaster.send((
                                task.url.to_string(),
                                downloaded_bytes,
                                total_size,
                            ));
                        }
                        Err(e) => {
                            task.status = DownloadStatus::Failed(DownloadError::Failed(
                                e.to_string(),
                            ));
                            had_error = true;
                        }
                    }
                }

                if !had_error {
                    match chunker::assemble_chunks(&task.destination_path, total_size).await {
                        Ok(_) => {
                            task.status = DownloadStatus::Completed;
                            let _ = progress_broadcaster.send((
                                task.url.to_string(),
                                total_size,
                                total_size,
                            ));
                        }
                        Err(e) => {
                            task.status =
                                DownloadStatus::Failed(DownloadError::FileSystem(e.to_string()));
                        }
                    }
                }
            }
        });
    }

    pub async fn queue_download(&self, url: Url, destination_path: PathBuf) -> Result<()> {
        let task = DownloadTask {
            url,
            destination_path,
            total_bytes: 0,
            downloaded_bytes: 0,
            status: DownloadStatus::Pending,
        };
        self.task_queue.send(task).await?;
        Ok(())
    }

    pub fn subscribe_progress(&self) -> watch::Receiver<(String, u64, u64)> {
        self.progress_broadcaster.subscribe()
    }
}
