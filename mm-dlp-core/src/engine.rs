use reqwest::Client;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::config::{DownloadConfig, DownloadResult, EngineConfig};
use crate::downloader::{build_http_client, fetch_with_fallback, stream_to_file};
use crate::error::EngineError;
use crate::extractor::PlatformRegistry;
use crate::processor::{download_cover_art, tag_file_in_place};
use crate::resolver::SourceResolver;
use crate::utils::{resolve_duplicate_path, sanitize_filename, sanitize_url, validate_output_path};

#[uniffi::export(callback_interface)]
pub trait DownloadProgressCallback: Send + Sync {
    fn on_status(&self, status: String);
    fn on_progress(&self, progress_percentage: f64);
    fn on_error(&self, error: EngineError);
    fn on_complete(&self, result: DownloadResult);
}

#[derive(uniffi::Object)]
pub struct MmDlpEngine {
    config: EngineConfig,
    client: Client,
    registry: Arc<PlatformRegistry>,
    resolver: SourceResolver,
    active_tasks: RwLock<HashMap<String, CancellationToken>>,
}

#[uniffi::export]
impl MmDlpEngine {
    #[uniffi::constructor]
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        let client = build_http_client()?;
        let registry = Arc::new(PlatformRegistry::new());
        let resolver = SourceResolver::new(registry.clone());
        Ok(Self {
            config,
            client,
            registry,
            resolver,
            active_tasks: RwLock::new(HashMap::new()),
        })
    }

    pub async fn execute_download(
        &self,
        task_id: String,
        raw_url: String,
        download_config: DownloadConfig,
        callback: Arc<dyn DownloadProgressCallback>,
    ) -> Result<DownloadResult, EngineError> {
        let start_time = Instant::now();
        let cancel_token = CancellationToken::new();
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let run_download = async {
            callback.on_status("Validating parameters".to_string());
            let clean_url = sanitize_url(&raw_url)?;
            let output_dir = validate_output_path(&download_config.output_dir)?;

            callback.on_status("Extracting metadata".to_string());
            let extractor = self
                .registry
                .route_url(&clean_url)
                .ok_or_else(|| EngineError::UnsupportedUrl(format!("No extractor for URL: {}", clean_url)))?;

            let metadata = extractor.extract_metadata(&self.client, &clean_url).await?;

            callback.on_status("Resolving audio stream".to_string());
            let candidate = if extractor.get_stream_url(&self.client, &metadata.id).await.is_ok() {
                extractor.get_stream_url(&self.client, &metadata.id).await?
            } else {
                self.resolver.resolve_best_stream(&self.client, &metadata).await?
            };

            callback.on_status("Fetching audio stream".to_string());
            let response = fetch_with_fallback(&self.client, &candidate.url).await?;

            let clean_title = sanitize_filename(&metadata.title);
            let target_filename = format!("{}.{}", clean_title, download_config.audio_format);
            let raw_target_path = output_dir.join(target_filename);
            let final_target_path = resolve_duplicate_path(&raw_target_path);

            callback.on_status("Downloading stream".to_string());
            let downloaded_bytes = stream_to_file(response, &final_target_path, cancel_token.clone()).await?;
            callback.on_progress(100.0);

            if let Some(art_url) = &metadata.thumbnail_url {
                callback.on_status("Fetching cover art".to_string());
                let cover_art = download_cover_art(&self.client, art_url).await;
                callback.on_status("Embedding metadata tags".to_string());
                let _ = tag_file_in_place(&final_target_path, &metadata, cover_art);
            }

            let elapsed_millis = start_time.elapsed().as_millis() as u64;
            let result = DownloadResult {
                file_path: final_target_path.to_string_lossy().to_string(),
                total_bytes: downloaded_bytes,
                elapsed_millis,
            };

            callback.on_complete(result.clone());
            Ok(result)
        };

        let result = run_download.await;
        {
            let mut tasks = self.active_tasks.write().await;
            tasks.remove(&task_id);
        }

        if let Err(ref err) = result {
            callback.on_error(err.clone());
        }

        result
    }

    pub async fn cancel_download(&self, task_id: &str) -> bool {
        let tasks = self.active_tasks.read().await;
        if let Some(token) = tasks.get(task_id) {
            token.cancel();
            true
        } else {
            false
        }
    }
}
