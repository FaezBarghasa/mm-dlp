use tokio::sync::{mpsc, watch};
use crate::network::quic_client::QuicHttpClient;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed(String),
}

pub struct DownloadTask {
    pub url: String,
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

        let client = manager.client;
        let progress_broadcaster = manager.progress_broadcaster.clone();

        tokio::spawn(async move {
            while let Some(task) = task_rx.recv().await {
                // Chunking and download logic will be implemented here
            }
        });

        Ok(manager)
    }

    pub async fn queue_download(&self, url: String, destination_path: PathBuf) -> Result<()> {
        let task = DownloadTask {
            url,
            destination_path,
            total_bytes: 0, // Will be determined later
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
