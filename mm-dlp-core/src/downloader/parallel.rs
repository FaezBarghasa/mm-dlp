use crate::client::EngineError;
use crate::downloader::manifest::DownloadSegment;
use bytes::Bytes;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct SequencedChunk {
    pub index: usize,
    pub data: Bytes,
}

impl PartialEq for SequencedChunk {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for SequencedChunk {}

impl PartialOrd for SequencedChunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SequencedChunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse the default max-heap ordering to create a min-heap structure based on index
        other.index.cmp(&self.index)
    }
}

pub async fn download_segments(
    client: Client,
    segments: Vec<DownloadSegment>,
    concurrency_limit: usize,
    tx: mpsc::Sender<Result<SequencedChunk, EngineError>>,
) {
    let semaphore = Arc::new(Semaphore::new(concurrency_limit));
    let mut join_set = JoinSet::new();

    for segment in segments {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let client_clone = client.clone();
        let tx_clone = tx.clone();

        join_set.spawn(async move {
            let _permit = permit;
            let response = client_clone.get(&segment.url).send().await;

            let result = match response {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => Ok(SequencedChunk { index: segment.index, data: bytes }),
                    Err(e) => Err(EngineError::OsApiError(format!("Failed to read body: {}", e))),
                },
                Err(e) => Err(EngineError::OsApiError(format!("Failed to download segment: {}", e))),
            };

            let _ = tx_clone.send(result).await;
        });
    }

    while let Some(_) = join_set.join_next().await {}
}