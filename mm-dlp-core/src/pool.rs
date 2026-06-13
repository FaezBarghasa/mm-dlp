use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use crate::platforms::PlatformRegistry;

#[derive(Debug, PartialEq, Eq)]
pub enum PoolError {
    QueueFull,
    QueueClosed,
}

pub struct DownloadJob {
    pub url: String,
}

/// A highly concurrent background worker pool for executing download tasks.
pub struct BackgroundPool {
    sender: mpsc::Sender<DownloadJob>,
}

impl BackgroundPool {
    /// Initializes the background thread pool manager.
    /// `concurrency_limit` bounds the simultaneous operations (e.g., 1 to 512).
    pub fn new(concurrency_limit: usize) -> Self {
        // Enforce the 1 to 512 multithreading rule
        let limit = concurrency_limit.clamp(1, 512);
        
        // Create a queue capable of holding pending jobs waiting for a worker
        let (tx, mut rx) = mpsc::channel::<DownloadJob>(10000);
        
        // Semaphore guarantees we never exceed the specified thread concurrency limit
        let semaphore = Arc::new(Semaphore::new(limit));
        let registry = Arc::new(PlatformRegistry::new());

        // The main dispatcher task - runs entirely detached in the background
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                // Acquire a concurrency permit. If `limit` tasks are already running, 
                // this gracefully waits until a thread becomes available.
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break, // Semaphore closed, shutting down gracefully
                };

                let registry_clone = Arc::clone(&registry);

                // Spawn the actual download job onto Tokio's multithreaded worker pool
                tokio::spawn(async move {
                    Self::process_job(job, registry_clone).await;
                    
                    // The permit is explicitly returned to the semaphore when dropped,
                    // freeing up a slot for the next queued download.
                    drop(permit);
                });
            }
        });

        Self { sender: tx }
    }

    /// Asynchronously submits a new job to the pool, waiting if the queue is temporarily full.
    pub async fn submit(&self, url: String) -> Result<(), PoolError> {
        self.sender
            .send(DownloadJob { url })
            .await
            .map_err(|_| PoolError::QueueClosed)
    }

    /// Contains the complete execution logic for a single download thread.
    async fn process_job(job: DownloadJob, registry: Arc<PlatformRegistry>) {
        match registry.extract(&job.url) {
            Ok(metadata) => {
                let start_time = std::time::Instant::now();
                println!(
                    "[START] Fetching {} media {} via {}...",
                    metadata.platform, metadata.media_type, metadata.media_id
                );
                
                // This is where you connect H3Impersonator. 
                // We simulate network latency here to prove concurrent execution.
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                
                println!(
                    "[SUCCESS] Downloaded {} in {}ms", 
                    metadata.media_id, start_time.elapsed().as_millis()
                );
            }
            Err(e) => {
                eprintln!("[ERROR] Unrecognized or invalid URL ({}): {:?}", job.url, e);
            }
        }
    }
}