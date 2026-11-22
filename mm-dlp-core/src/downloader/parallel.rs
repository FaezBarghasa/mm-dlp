use crate::downloader::manifest::DownloadSegment;
use crate::error::EngineError;
use bytes::Bytes;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
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
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };
        let client_clone = client.clone();
        let tx_clone = tx.clone();

        join_set.spawn(async move {
            let _permit = permit;
            let response = client_clone.get(&segment.url).send().await;

            let result = match response {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => Ok(SequencedChunk {
                        index: segment.index,
                        data: bytes,
                    }),
                    Err(e) => Err(EngineError::IoError(format!("Failed to read body: {}", e))),
                },
                Err(e) => Err(EngineError::IoError(format!(
                    "Failed to download segment: {}",
                    e
                ))),
            };

            let _ = tx_clone.send(result).await;
        });
    }

    while join_set.join_next().await.is_some() {}
}

/// Exponential Weighted Moving Average (EWMA) bandwidth estimator for adaptive jitter buffering.
#[derive(Debug, Clone)]
pub struct EwmaBandwidthEstimator {
    alpha: f64,
    estimated_bytes_per_sec: f64,
}

impl EwmaBandwidthEstimator {
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha: alpha.clamp(0.01, 0.99),
            estimated_bytes_per_sec: 0.0,
        }
    }

    pub fn update(&mut self, bytes: u64, elapsed_secs: f64) {
        if elapsed_secs <= 0.0 || bytes == 0 {
            return;
        }
        let sample = bytes as f64 / elapsed_secs;
        if self.estimated_bytes_per_sec == 0.0 {
            self.estimated_bytes_per_sec = sample;
        } else {
            self.estimated_bytes_per_sec =
                self.alpha * sample + (1.0 - self.alpha) * self.estimated_bytes_per_sec;
        }
    }

    pub fn current_estimate(&self) -> f64 {
        self.estimated_bytes_per_sec
    }

    /// Calculate an adaptive pacing sleep delay to smooth downstream jitter.
    pub fn calculate_jitter_delay(
        &self,
        chunk_size: usize,
        target_buffer_secs: f64,
    ) -> std::time::Duration {
        if self.estimated_bytes_per_sec <= 0.0 {
            return std::time::Duration::from_millis(0);
        }
        let time_to_transfer = chunk_size as f64 / self.estimated_bytes_per_sec;
        if time_to_transfer < target_buffer_secs / 100.0 {
            std::time::Duration::from_millis(5)
        } else {
            std::time::Duration::from_millis(0)
        }
    }
}

impl Default for EwmaBandwidthEstimator {
    fn default() -> Self {
        Self::new(0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ewma_bandwidth_estimator() {
        let mut estimator = EwmaBandwidthEstimator::new(0.2);
        assert_eq!(estimator.current_estimate(), 0.0);

        // Initial sample: 1,000,000 bytes in 1.0 sec = 1,000,000 B/s
        estimator.update(1_000_000, 1.0);
        assert_eq!(estimator.current_estimate(), 1_000_000.0);

        // Second sample: 2,000,000 bytes in 1.0 sec = 2,000,000 B/s
        // EWMA = 0.2 * 2,000,000 + 0.8 * 1,000,000 = 1,200,000 B/s
        estimator.update(2_000_000, 1.0);
        assert_eq!(estimator.current_estimate(), 1_200_000.0);

        let delay = estimator.calculate_jitter_delay(1024, 10.0);
        assert!(delay.as_millis() <= 5);
    }
}
