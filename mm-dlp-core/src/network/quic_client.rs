use std::sync::Arc;
use std::net::SocketAddr;
use anyhow::{anyhow, Result};
use h3::client::RequestStream;
use h3_quinn::Connection;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rustls::{Certificate, RootCertStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use url::Url;
use bytes::Bytes;

pub enum HttpClient {
    Http3(RequestStream),
    Http2(reqwest::Response),
}

pub struct QuicHttpClient {
    endpoint: Endpoint,
    http2_client: reqwest::Client,
}

impl QuicHttpClient {
    pub fn new() -> Result<Self> {
        let mut roots = RootCertStore::empty();
        let certs = rustls_native_certs::load_native_certs()?;
        for cert in certs {
            roots.add(&Certificate(cert.0))?;
        }

        let client_crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(roots)
            .with_no_client_auth();

        let mut endpoint = Endpoint::client("[::]:0".parse()?)?;
        let mut transport_config = TransportConfig::default();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(30).try_into()?));
        let client_config = ClientConfig::new(Arc::new(client_crypto))
            .transport_config(Arc::new(transport_config));
        endpoint.set_default_client_config(client_config);

        Ok(Self {
            endpoint,
            http2_client: reqwest::Client::new(),
        })
    }

    pub async fn get(&self, url: &Url) -> Result<HttpClient> {
        let host = url.host_str().ok_or_else(|| anyhow!("URL has no host"))?;
        let port = url.port_or_known_default().unwrap_or(443);
        let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

        // Attempt QUIC connection
        if let Ok(Ok(conn)) = timeout(Duration::from_secs(5), self.endpoint.connect(addr, host)).await {
            if let Ok(h3_conn) = h3_quinn::Connection::new(conn).await {
                 return self.send_h3_request(h3_conn, url).await;
            }
        }

        // Fallback to HTTP/2
        self.send_http2_request(url).await
    }

    async fn send_h3_request(&self, mut h3_conn: Connection, url: &Url) -> Result<HttpClient> {
        let request = http::Request::builder()
            .method("GET")
            .uri(url.as_str())
            .body(())?;

        let stream = h3_conn.send_request(request).await?;
        Ok(HttpClient::Http3(stream))
    }

    async fn send_http2_request(&self, url: &Url) -> Result<HttpClient> {
        let response = self.http2_client.get(url.clone()).send().await?;
        Ok(HttpClient::Http2(response))
    }
}
