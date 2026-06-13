use crate::client::EngineError;
use quiche;
use std::net::{SocketAddr, ToSocketAddrs};
use tokio::net::UdpSocket;

pub const CHROME_H3_MAX_IDLE_TIMEOUT: u64 = 30000;
pub const CHROME_H3_INITIAL_MAX_DATA: u64 = 10_485_760;
pub const CHROME_H3_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u64 = 1_048_576;
pub const CHROME_H3_INITIAL_MAX_STREAMS_BIDI: u64 = 100;

pub struct H3Impersonator {
    quic_config: quiche::Config,
    h3_config: quiche::h3::Config,
}

impl H3Impersonator {
    pub fn new() -> Result<Self, EngineError> {
        let mut quic_config = quiche::Config::new(quiche::PROTOCOL_VERSION)
            .map_err(|e| EngineError::OsApiError(format!("Failed to create QUIC config: {}", e)))?;

        // Chrome-like QUIC parameters for fingerprint evasion
        quic_config.set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
            .map_err(|e| EngineError::OsApiError(format!("Failed to set H3 ALPN: {}", e)))?;
        quic_config.set_max_idle_timeout(CHROME_H3_MAX_IDLE_TIMEOUT);
        quic_config.set_initial_max_data(CHROME_H3_INITIAL_MAX_DATA);
        quic_config.set_initial_max_stream_data_bidi_local(CHROME_H3_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL);
        quic_config.set_initial_max_stream_data_bidi_remote(CHROME_H3_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL);
        quic_config.set_initial_max_stream_data_uni(CHROME_H3_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL);
        quic_config.set_initial_max_streams_bidi(CHROME_H3_INITIAL_MAX_STREAMS_BIDI);
        quic_config.set_initial_max_streams_uni(CHROME_H3_INITIAL_MAX_STREAMS_BIDI);
        
        // Enable active migration to support connection recovery if the network interface/IP changes
        quic_config.set_disable_active_migration(false);

        // Impersonate Chrome's GREASE for QUIC connections
        quic_config.set_grease(true);

        let h3_config = quiche::h3::Config::new()
            .map_err(|e| EngineError::OsApiError(format!("Failed to create H3 config: {}", e)))?;

        Ok(Self {
            quic_config,
            h3_config,
        })
    }

    pub fn get_quic_config(&mut self) -> &mut quiche::Config {
        &mut self.quic_config
    }

    pub fn get_h3_config(&mut self) -> &mut quiche::h3::Config {
        &mut self.h3_config
    }

    /// Binds an IPv6-first UDP socket for QUIC traffic. Safely falls back to IPv4.
    pub async fn bind_dual_stack_socket() -> Result<UdpSocket, EngineError> {
        // Attempt to bind to the IPv6 wildcard first to support IPv6 natively
        match UdpSocket::bind("[::]:0").await {
            Ok(socket) => Ok(socket),
            Err(_) => {
                // Safe fallback to IPv4 if the host system network does not support IPv6 routing
                UdpSocket::bind("0.0.0.0:0").await
                    .map_err(|e| EngineError::OsApiError(format!("Failed to bind UDP socket: {}", e)))
            }
        }
    }

    /// Resolves a hostname to a SocketAddr, preferring IPv6 (AAAA) records over IPv4.
    pub fn resolve_ipv6_preferred(host: &str, port: u16) -> Result<SocketAddr, EngineError> {
        let addrs = format!("{}:{}", host, port)
            .to_socket_addrs()
            .map_err(|e| EngineError::OsApiError(format!("DNS resolution failed: {}", e)))?;

        let mut ipv4_fallback = None;

        for addr in addrs {
            if addr.is_ipv6() {
                return Ok(addr);
            } else if ipv4_fallback.is_none() {
                ipv4_fallback = Some(addr);
            }
        }

        ipv4_fallback.ok_or_else(|| EngineError::OsApiError("No valid IP addresses found".to_string()))
    }

    /// Formats request headers for HTTP/3 exactly matching Chrome's internal ordering.
    /// Includes support for a download recovery option via `resume_offset`.
    pub fn format_request_headers(
        method: &str,
        authority: &str,
        scheme: &str,
        path: &str,
        resume_offset: Option<u64>,
    ) -> Vec<quiche::h3::Header> {
        let mut headers = vec![
            quiche::h3::Header::new(b":method", method.as_bytes()),
            quiche::h3::Header::new(b":authority", authority.as_bytes()),
            quiche::h3::Header::new(b":scheme", scheme.as_bytes()),
            quiche::h3::Header::new(b":path", path.as_bytes()),
        ];

        // Application-layer download recovery
        if let Some(offset) = resume_offset {
            let range_val = format!("bytes={}-", offset);
            headers.push(quiche::h3::Header::new(b"range", range_val.as_bytes()));
        }

        headers
    }

    /// Initializes a quiche connection tailored for the dual-stack IPv6 socket,
    /// with an optional session data parameter for connection resumption recovery.
    pub async fn connect_quic(
        &mut self,
        host: &str,
        port: u16,
        session_data: Option<&[u8]>,
    ) -> Result<(quiche::Connection, UdpSocket), EngineError> {
        let socket = Self::bind_dual_stack_socket().await?;
        let peer_addr = Self::resolve_ipv6_preferred(host, port)?;

        // Generate a localized SCID (Source Connection ID) 
        let mut scid = [0; quiche::MAX_CONN_ID_LEN];
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        
        for (i, byte) in scid.iter_mut().enumerate() {
            *byte = ((timestamp >> (i % 8 * 8)) & 0xFF) as u8;
        }

        let local_addr = socket.local_addr()
            .map_err(|e| EngineError::OsApiError(format!("Failed to get local addr: {}", e)))?;

        let mut conn = quiche::connect(
            Some(host),
            &scid,
            local_addr,
            peer_addr,
            &mut self.quic_config,
        ).map_err(|e| EngineError::OsApiError(format!("QUIC connection initialization failed: {}", e)))?;

        // Apply QUIC session ticket for 0-RTT download connection recovery
        if let Some(session) = session_data {
            conn.set_session(session)
                .map_err(|e| EngineError::OsApiError(format!("Failed to restore QUIC session: {}", e)))?;
        }

        Ok((conn, socket))
    }
}

impl Default for H3Impersonator {
    fn default() -> Self {
        Self::new().expect("Failed to initialize H3Impersonator")
    }
}