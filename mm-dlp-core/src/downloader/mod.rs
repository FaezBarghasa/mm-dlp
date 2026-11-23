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
    build_http_client_with_config(&crate::config::EngineConfig::default())
}

pub fn build_http_client_with_config(
    config: &crate::config::EngineConfig,
) -> Result<Client, EngineError> {
    #[cfg(target_os = "ios")]
    let (max_idle, idle_timeout) = (8, Duration::from_secs(30));
    #[cfg(not(target_os = "ios"))]
    let (max_idle, idle_timeout) = (32, Duration::from_secs(90));

    let ua = if config.user_agent != "mm-dlp/1.0.0" {
        config.user_agent.clone()
    } else if config.is_ios_safari() {
        crate::config::SAFARI_IOS_USER_AGENT.to_string()
    } else if config.is_safari_or_webkit() {
        crate::config::SAFARI_MACOS_USER_AGENT.to_string()
    } else if config.is_gecko_or_firefox() {
        crate::config::FIREFOX_GECKO_USER_AGENT.to_string()
    } else {
        crate::config::CHROME_USER_AGENT.to_string()
    };

    let mut builder = Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds.max(5)))
        .connect_timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .pool_max_idle_per_host(max_idle)
        .pool_idle_timeout(idle_timeout)
        .user_agent(&ua);

    if config.is_gecko_or_firefox() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            reqwest::header::HeaderValue::from_static("en-US,en;q=0.5"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-dest"),
            reqwest::header::HeaderValue::from_static("document"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-mode"),
            reqwest::header::HeaderValue::from_static("navigate"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-site"),
            reqwest::header::HeaderValue::from_static("none"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-user"),
            reqwest::header::HeaderValue::from_static("?1"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("upgrade-insecure-requests"),
            reqwest::header::HeaderValue::from_static("1"),
        );
        builder = builder.default_headers(headers);
    } else if config.is_safari_or_webkit() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-dest"),
            reqwest::header::HeaderValue::from_static("document"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-mode"),
            reqwest::header::HeaderValue::from_static("navigate"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("sec-fetch-site"),
            reqwest::header::HeaderValue::from_static("none"),
        );
        builder = builder.default_headers(headers);
    }

    // Configure Cookie Jar if cookies_file or cookies_from_browser is specified
    let mut cookie_items = Vec::new();
    if let Some(ref path) = config.cookies_file
        && let Ok(content) = std::fs::read_to_string(path)
    {
        cookie_items
            .extend(crate::client::cookies::CookieExtractor::parse_netscape_cookie_items(&content));
    }
    if let Some(ref browser_spec) = config.cookies_from_browser
        && let Ok(content) = crate::client::cookies::BrowserCookieExtractor::extract(browser_spec)
    {
        cookie_items
            .extend(crate::client::cookies::CookieExtractor::parse_netscape_cookie_items(&content));
    }

    if !cookie_items.is_empty() {
        let cookie_jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
        for cookie in &cookie_items {
            let scheme = if cookie.secure { "https" } else { "http" };
            let domain_clean = cookie.domain.trim_start_matches('.');
            let path_clean = if cookie.path.is_empty() {
                "/"
            } else {
                &cookie.path
            };
            let url_str = format!("{}://{}{}", scheme, domain_clean, path_clean);
            if let Ok(url) = url_str.parse::<reqwest::Url>() {
                let cookie_str = format!("{}={}", cookie.name, cookie.value);
                cookie_jar.add_cookie_str(&cookie_str, &url);
            }
        }
        builder = builder.cookie_provider(cookie_jar);
    }

    if let Some(ref proxy_url) = config.proxy
        && let Ok(proxy) = reqwest::Proxy::all(proxy_url)
    {
        builder = builder.proxy(proxy);
    }

    // Configure DNS resolver: default to 1.1.1.1 with fallback to 9.9.9.9,
    // or custom DNS servers if specified.
    let dns_addrs = if let Some(ref dns_str) = config.dns_servers {
        crate::client::dns::parse_dns_servers(dns_str)
            .unwrap_or_else(|_| crate::client::dns::default_dns_servers())
    } else {
        crate::client::dns::default_dns_servers()
    };
    let dns_resolver = std::sync::Arc::new(crate::client::dns::CustomDnsResolver::new(dns_addrs));
    builder = builder.dns_resolver(dns_resolver);

    builder
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
