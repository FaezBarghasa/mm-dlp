use tokio::sync::{mpsc, watch};
use crate::network::quic_client::QuicHttpClient;
use crate::download::chunker::{self, calculate_chunks};
use anyhow::Result;
use std::path::{Path, PathBuf};
use thiserror::Error;
use url::Url;
use futures::stream::{self, StreamExt};

#[derive(Error, Debug, Clone)]
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

pub struct DownloadTask {
    pub url: Url,
    pub destination_path: PathBuf,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
}

pub struct DownloadManager {
    client: QuicHttpClient,
    task_queue: mpsc::Sender<DownloadTask>,
    progress_broadcaster: watch::Sender<(String, u64, u64)>, // url, downloaded, total
}

impl DownloadManager {
    pub fn new() -> Result<Self> {
        let (task_tx, mut task_rx) = mpsc::channel(100);
        let (progress_tx, _) = watch::channel(("".to_string(), 0, 0));

        let manager = Self {
            client: QuicHttpClient::new()?,
            task_queue: task_tx,
            progress_broadcaster: progress_tx,
        };

        let client = manager.client.clone();
        let progress_broadcaster = manager.progress_broadcaster.clone();

        tokio::spawn(async move {
            while let Some(mut task) = task_rx.recv().await {
                task.status = DownloadStatus::Downloading;
                let _ = progress_broadcaster.send((task.url.to_string(), task.downloaded_bytes, task.total_bytes));

                let head_resp = client.get(&task.url).await.unwrap();
                let total_size = match head_resp {
                    crate::network::quic_client::HttpClient::Http2(resp) => resp.content_length().unwrap_or(0),
                    _ => 0, // H3 does not easily expose content-length in this setup
                };
                task.total_bytes = total_size;

                let part_dir = task.destination_path.with_extension("part");
                if !part_dir.exists() {
                    tokio::fs::create_dir_all(&part_dir).await.unwrap();
                }

                let chunks = calculate_chunks(total_size);
                let mut downloaded_bytes = 0;

                let chunk_downloads = stream::iter(chunks)
                    .map(|(start, end)| {
                        let client = client.clone();
                        let url = task.url.clone();
                        let part_dir = part_dir.clone();
                        async move {
                            let chunk_path = part_dir.join(format!("chunk_{}", start / chunker::CHUNK_SIZE));
                            if chunk_path.exists() {
                                return Ok((start, tokio::fs::metadata(&chunk_path).await.unwrap().len()));
                            }
                            let chunk = chunker::download_chunk(&client, &url, start, end).await?;
                            tokio::fs::write(&chunk_path, &chunk).await?;
                            Ok((start, chunk.len() as u64))
                        }
                    })
                    .buffer_unordered(8); // Concurrency limit

                chunk_downloads
                    .for_each(|result| {
                        match result {
                            Ok((_, size)) => {
                                downloaded_bytes += size;
                                let _ = progress_broadcaster.send((task.url.to_string(), downloaded_bytes, total_size));
                            }
                            Err(e) => {
                                task.status = DownloadStatus::Failed(DownloadError::Failed(e.to_string()));
                                let _ = progress_broadcaster.send((task.url.to_string(), downloaded_bytes, total_size));
                            }
                        }
                        async {}
                    })
                    .await;

                if let DownloadStatus::Downloading = task.status {
                    match chunker::assemble_chunks(&task.destination_path, total_size).await {
                        Ok(_) => {
                            task.status = DownloadStatus::Completed;
                            let _ = progress_broadcaster.send((task.url.to_string(), total_size, total_size));
                        }
                        Err(e) => {
                            task.status = DownloadStatus::Failed(DownloadError::FileSystem(e.to_string()));
                            let _ = progress_broadcaster.send((task.url.to_string(), downloaded_bytes, total_size));
                        }
                    }
                }
            }
        });

        Ok(manager)
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
