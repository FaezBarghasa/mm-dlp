//! Manages a highly concurrent, bounded pool of background workers for executing download tasks.
//!
//! This module provides a `BackgroundPool` that allows submitting download jobs
//! from any context, ensuring that no more than a specified number of downloads
//! run simultaneously. It's designed for efficiency and graceful handling of
//! backpressure.

use crate::platforms::PlatformRegistry;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

/// Represents errors that can occur when submitting a job to the `BackgroundPool`.
#[derive(Debug, PartialEq, Eq)]
pub enum PoolError {
    /// The job queue is full and cannot accept new jobs at this time.
    QueueFull,
    /// The worker pool has been shut down and can no longer accept jobs.
    QueueClosed,
}

/// A unit of work representing a single URL to be downloaded.
pub struct DownloadJob {
    /// The URL of the media to be downloaded.
    pub url: String,
}

/// A highly concurrent background worker pool for executing download tasks.
///
/// It uses a bounded channel to queue incoming jobs and a semaphore to limit
/// the number of concurrently running workers, preventing resource exhaustion.
pub struct BackgroundPool {
    /// The sender half of the MPSC channel used to queue download jobs.
    sender: mpsc::Sender<DownloadJob>,
}

impl BackgroundPool {
    /// Initializes the background thread pool manager.
    ///
    /// # Arguments
    ///
    /// * `concurrency_limit` - The maximum number of downloads to run in parallel.
    ///   This value is clamped between 1 and 512 to ensure reasonable bounds.
    pub fn new(concurrency_limit: usize) -> Self {
        // Enforce the 1 to 512 multithreading rule for stability.
        let limit = concurrency_limit.clamp(1, 512);

        // Create a bounded MPSC channel to act as the job queue.
        let (tx, mut rx) = mpsc::channel::<DownloadJob>(10000);

        // A semaphore ensures we never exceed the specified concurrency limit.
        let semaphore = Arc::new(Semaphore::new(limit));
        let registry = Arc::new(PlatformRegistry::new());

        // The main dispatcher task, running detached in the background.
        tokio::spawn(async move {
            // Continuously listen for incoming jobs.
            while let Some(job) = rx.recv().await {
                // Acquire a permit from the semaphore. If the concurrency limit is reached,
                // this will asynchronously wait until a worker finishes.
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break, // Semaphore closed, indicating a graceful shutdown.
                };

                let registry_clone = Arc::clone(&registry);

                // Spawn the actual download job onto Tokio's multithreaded worker pool.
                tokio::spawn(async move {
                    Self::process_job(job, registry_clone).await;

                    // The permit is automatically returned to the semaphore when `_permit`
                    // goes out of scope, freeing up a slot for the next queued download.
                    drop(permit);
                });
            }
        });

        Self { sender: tx }
    }

    /// Asynchronously submits a new job to the pool.
    ///
    /// If the job queue is full, this method will wait until space is available.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL of the media to download.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the job was successfully submitted.
    /// * `Err(PoolError::QueueClosed)` if the pool has been shut down.
    pub async fn submit(&self, url: String) -> Result<(), PoolError> {
        self.sender
            .send(DownloadJob { url })
            .await
            .map_err(|_| PoolError::QueueClosed)
    }

    /// Contains the complete execution logic for a single download job.
    ///
    /// This function is responsible for extracting metadata, performing the download,
    /// and handling any potential errors.
    async fn process_job(job: DownloadJob, registry: Arc<PlatformRegistry>) {
        match registry.extract(&job.url) {
            Ok(metadata) => {
                let start_time = std::time::Instant::now();
                println!(
                    "[START] Fetching {} media {} via {}...",
                    metadata.platform, metadata.media_type, metadata.media_id
                );

                // TODO: Replace this simulation with the actual H3Impersonator network client.
                // We simulate network latency here to demonstrate concurrent execution.
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                println!(
                    "[SUCCESS] Downloaded {} in {}ms",
                    metadata.media_id,
                    start_time.elapsed().as_millis()
                );
            }
            Err(e) => {
                eprintln!("[ERROR] Unrecognized or invalid URL ({}): {:?}", job.url, e);
            }
        }
    }
}
