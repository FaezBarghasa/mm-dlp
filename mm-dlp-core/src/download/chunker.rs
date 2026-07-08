use crate::network::quic_client::{HttpClient, QuicHttpClient};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::time::{sleep, Duration};
use url::Url;

const CHUNK_SIZE: u64 = 1024 * 1024 * 4; // 4MB

pub async fn download_chunk(
    client: &QuicHttpClient,
    url: &Url,
    start: u64,
    end: u64,
) -> Result<bytes::Bytes> {
    let mut last_error: Option<anyhow::Error> = None;
    for i in 0..3 {
        let request_url = url.clone();
        let mut request = client.get(&request_url).await?;

        let mut response = match request {
            HttpClient::Http3(mut stream) => {
                let h3_request = http::Request::builder()
                    .method("GET")
                    .uri(request_url.as_str())
                    .header("Range", format!("bytes={}-{}", start, end))
                    .body(())?;
                stream.send_request(h3_request).await?;
                stream.recv_response().await?
            }
            HttpClient::Http2(resp) => {
                let client = reqwest::Client::new();
                client.get(request_url)
                    .header("Range", format!("bytes={}-{}", start, end))
                    .send()
                    .await?
                    .error_for_status()?
            }
        };

        let mut buffer = Vec::new();
        match response {
            HttpClient::Http3(mut stream) => {
                while let Some(chunk) = stream.recv_data().await? {
                    buffer.extend_from_slice(&chunk);
                }
            },
            HttpClient::Http2(mut resp) => {
                while let Some(chunk) = resp.chunk().await? {
                    buffer.extend_from_slice(&chunk);
                }
            }
        }
        return Ok(bytes::Bytes::from(buffer));

        sleep(Duration::from_secs(2u64.pow(i))).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Chunk download failed after 3 retries")))
}

pub async fn assemble_chunks(
    destination_path: &Path,
    total_size: u64,
) -> Result<()> {
    let part_path = destination_path.with_extension("part");
    let mut final_file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&destination_path)
        .await?;

    for i in 0..(total_size / CHUNK_SIZE) + 1 {
        let chunk_path = part_path.join(format!("chunk_{}", i));
        let mut chunk_file = File::open(&chunk_path).await?;
        let mut buffer = Vec::new();
        chunk_file.read_to_end(&mut buffer).await?;
        final_file.write_all(&buffer).await?;
        fs::remove_file(&chunk_path).await?;
    }

    fs::remove_dir_all(&part_path).await?;
    Ok(())
}

pub fn calculate_chunks(total_size: u64) -> Vec<(u64, u64)> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < total_size {
        let end = (start + CHUNK_SIZE - 1).min(total_size - 1);
        chunks.push((start, end));
        start += CHUNK_SIZE;
    }
    chunks
}
