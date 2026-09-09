use reqwest::{Client, Response};
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::EngineError;

pub mod flusher;
pub mod manifest;
pub mod mmap;
pub mod parallel;

pub fn build_http_client() -> Result<Client, EngineError> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .user_agent("mm-dlp/1.0.0")
        .build()
        .map_err(|e| EngineError::Network(format!("Failed to build reqwest client: {}", e)))
}

pub async fn fetch_with_fallback(client: &Client, url: &str) -> Result<Response, EngineError> {
    let quic_future = client.get(url).header("Alt-Svc", "h3=\":443\"").send();
    
    match tokio::time::timeout(Duration::from_secs(10), quic_future).await {
        Ok(Ok(resp)) if resp.status().is_success() => Ok(resp),
        _ => {
            let standard_resp = client.get(url).send().await?;
            if standard_resp.status().is_success() {
                Ok(standard_resp)
            } else {
                Err(EngineError::Network(format!(
                    "HTTP download failed with status code: {}",
                    standard_resp.status()
                )))
            }
        }
    }
}

pub async fn stream_to_file(
    mut response: Response,
    target_path: &Path,
    cancel_token: CancellationToken,
) -> Result<u64, EngineError> {
    let tmp_path = target_path.with_extension("tmp");
    let mut file = File::create(&tmp_path).await?;
    let mut downloaded_bytes: u64 = 0;

    while let Some(chunk_res) = response.chunk().await.map_err(|e| EngineError::Network(e.to_string()))? {
        if cancel_token.is_cancelled() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(EngineError::Cancelled);
        }
        file.write_all(&chunk_res).await?;
        downloaded_bytes += chunk_res.len() as u64;
    }

    file.flush().await?;
    file.sync_all().await?;

    tokio::fs::rename(&tmp_path, target_path).await?;
    Ok(downloaded_bytes)
}