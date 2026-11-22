use reqwest::{Client, Response};
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::EngineError;

pub mod dash;
pub mod flusher;
pub mod hls;
pub mod manifest;
pub mod mmap;
pub mod parallel;
pub mod torrent;

pub fn build_http_client() -> Result<Client, EngineError> {
    #[cfg(target_os = "ios")]
    let (max_idle, idle_timeout) = (8, Duration::from_secs(30));
    #[cfg(not(target_os = "ios"))]
    let (max_idle, idle_timeout) = (32, Duration::from_secs(90));

    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .pool_max_idle_per_host(max_idle)
        .pool_idle_timeout(idle_timeout)
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
    if target_path == Path::new("-") {
        let mut stdout = tokio::io::stdout();
        let mut downloaded_bytes: u64 = 0;

        while let Some(chunk_res) = response
            .chunk()
            .await
            .map_err(|e| EngineError::Network(e.to_string()))?
        {
            if cancel_token.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            stdout.write_all(&chunk_res).await?;
            downloaded_bytes += chunk_res.len() as u64;
        }

        stdout.flush().await?;
        return Ok(downloaded_bytes);
    }

    let tmp_path = target_path.with_extension("tmp");
    let mut file = File::create(&tmp_path).await?;
    let mut downloaded_bytes: u64 = 0;

    while let Some(chunk_res) = response
        .chunk()
        .await
        .map_err(|e| EngineError::Network(e.to_string()))?
    {
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

#[cfg(target_os = "linux")]
pub mod splice {
    use std::io;
    use std::os::unix::io::RawFd;

    /// Perform a zero-copy Linux splice operation forwarding bytes directly between pipes.
    ///
    /// # Clarification & Linux Constraints
    /// Linux `splice(2)` requires at least one file descriptor to refer to a pipe.
    /// When transferring data from a TLS/QUIC network socket, user-space decryption
    /// is mandatory, so socket-to-stdout splice is not feasible without kernel kTLS.
    /// This function performs genuine in-kernel zero-copy page forwarding between two OS pipes
    /// (e.g., from an internal buffer pipe into standard output or another IPC pipe).
    ///
    /// # Safety
    /// Requires valid file descriptors `in_pipe_fd` and `out_pipe_fd`.
    pub unsafe fn splice_pipe_to_pipe(
        in_pipe_fd: RawFd,
        out_pipe_fd: RawFd,
        len: usize,
    ) -> io::Result<usize> {
        let ret = unsafe {
            libc::splice(
                in_pipe_fd,
                std::ptr::null_mut(),
                out_pipe_fd,
                std::ptr::null_mut(),
                len,
                libc::SPLICE_F_MOVE | libc::SPLICE_F_MORE,
            )
        };
        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(ret as usize)
        }
    }
}
