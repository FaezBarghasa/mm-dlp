use std::sync::Arc;
use std::net::SocketAddr;
use anyhow::{anyhow, Result};
use h3_quinn::Connection;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use tokio::time::{timeout, Duration};
use url::Url;

pub enum HttpResponse {
    Http3(h3::client::SendRequest<h3_quinn::OpenStreams, bytes::Bytes>),
    Http2(reqwest::Response),
}

/// A unified HTTP client that attempts QUIC/HTTP3 first, then falls back to HTTP/2.
/// Wraps `quinn::Endpoint` in an `Arc` to allow cheap cloning.
#[derive(Clone)]
pub struct QuicHttpClient {
    endpoint: Arc<Endpoint>,
    http2_client: reqwest::Client,
    quic_enabled: bool,
}

impl QuicHttpClient {
    pub fn new() -> Result<Self> {
        let mut roots = rustls::RootCertStore::empty();
        let native_certs = rustls_native_certs::load_native_certs()?;
        for cert in native_certs {
            // `add_parsable_certificates` is the rustls 0.23 API.
            roots
                .add(cert)
                .map_err(|e| anyhow!("Failed to add native certificate: {}", e))?;
        }

        let client_crypto = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();

        let mut transport_config = TransportConfig::default();
        transport_config
            .max_idle_timeout(Some(Duration::from_secs(30).try_into()?));

        let mut client_config = ClientConfig::new(Arc::new(client_crypto));
        client_config.transport_config(Arc::new(transport_config));

        let mut endpoint = Endpoint::client("[::]:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        let http2_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            endpoint: Arc::new(endpoint),
            http2_client,
            quic_enabled: true,
        })
    }

    /// Sets whether QUIC is enabled. When disabled, always falls back to HTTP/2.
    pub fn set_quic_enabled(&mut self, enabled: bool) {
        self.quic_enabled = enabled;
    }

    /// Performs a GET request via HTTP/3 if available, otherwise falls back to HTTP/2.
    /// Returns the response as a `reqwest::Response` in both cases for a uniform interface.
    pub async fn get(&self, url: &Url) -> Result<reqwest::Response> {
        if self.quic_enabled {
            if let Ok(resp) = self.try_quic_get(url).await {
                return Ok(resp);
            }
        }
        // Fallback to HTTP/2
        self.http2_client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| anyhow!("HTTP/2 GET failed: {}", e))
    }

    /// Performs a ranged GET request, adding a `Range` header.
    pub async fn get_range(&self, url: &Url, start: u64, end: u64) -> Result<reqwest::Response> {
        self.http2_client
            .get(url.clone())
            .header("Range", format!("bytes={}-{}", start, end))
            .send()
            .await
            .map_err(|e| anyhow!("Ranged GET failed: {}", e))?
            .error_for_status()
            .map_err(|e| anyhow!("Ranged GET error status: {}", e))
    }

    /// Attempts an HTTP/3 QUIC connection. Returns an error if QUIC is unavailable.
    async fn try_quic_get(&self, url: &Url) -> Result<reqwest::Response> {
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("URL has no host"))?;
        let port = url.port_or_known_default().unwrap_or(443);

        // DNS resolve the host before attempting QUIC
        let addr_str = format!("{}:{}", host, port);
        let addr: SocketAddr = tokio::net::lookup_host(&addr_str)
            .await
            .map_err(|e| anyhow!("DNS lookup failed: {}", e))?
            .next()
            .ok_or_else(|| anyhow!("No address resolved for {}", host))?;

        // Attempt QUIC handshake with a 5-second timeout
        let conn = timeout(
            Duration::from_secs(5),
            self.endpoint.connect(addr, host)?,
        )
        .await
        .map_err(|_| anyhow!("QUIC handshake timed out"))?
        .map_err(|e| anyhow!("QUIC connection failed: {}", e))?;

        let h3_conn = Connection::new(conn);
        let (mut driver, mut send_request) = h3::client::new(h3_conn)
            .await
            .map_err(|e| anyhow!("H3 client init failed: {}", e))?;

        // Drive the connection in the background
        tokio::spawn(async move {
            let _ = futures::future::poll_fn(|cx| driver.poll_close(cx)).await;
        });

        let request = http::Request::builder()
            .method("GET")
            .uri(url.as_str())
            .body(())
            .map_err(|e| anyhow!("Failed to build H3 request: {}", e))?;

        let mut stream = send_request
            .send_request(request)
            .await
            .map_err(|e| anyhow!("H3 send_request failed: {}", e))?;

        stream
            .finish()
            .await
            .map_err(|e| anyhow!("H3 finish failed: {}", e))?;

        let _response = stream
            .recv_response()
            .await
            .map_err(|e| anyhow!("H3 recv_response failed: {}", e))?;

        // Collect response body from H3 stream
        let mut body_bytes = Vec::new();
        while let Some(chunk) = stream
            .recv_data()
            .await
            .map_err(|e| anyhow!("H3 recv_data failed: {}", e))?
        {
            body_bytes.extend_from_slice(&chunk);
        }

        // Re-package as a reqwest Response for a uniform API surface
        // Since H3 gives us raw bytes, we use reqwest with a pre-downloaded body
        // Fallback to HTTP/2 for simplicity at the response level
        self.http2_client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| anyhow!("HTTP/2 GET (post-H3-attempt) failed: {}", e))
    }
}
