use boring::ssl::{SslConnector, SslMethod, SslOptions, SslVersion};
use crate::client::EngineError;

pub struct TlsImpersonator {
    connector: SslConnector,
}

impl TlsImpersonator {
    pub fn new() -> Result<Self, EngineError> {
        let mut builder = SslConnector::builder(SslMethod::tls_client())
            .map_err(|e| EngineError::OsApiError(format!("Failed to create SSL builder: {}", e)))?;

        // Configure standard ciphers: TLS 1.2 ciphers for compatibility (TLS 1.3 ciphers are configured by default in BoringSSL)
        let cipher_list = "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305";
        builder.set_cipher_list(cipher_list)
            .map_err(|e| EngineError::OsApiError(format!("Failed to set cipher list: {}", e)))?;

        // Enable TLS 1.3 and GREASE for advanced impersonation
        builder.set_min_proto_version(Some(SslVersion::TLS1_3))
            .map_err(|e| EngineError::OsApiError(format!("Failed to set min protocol version: {}", e)))?;
        builder.set_max_proto_version(Some(SslVersion::TLS1_3))
            .map_err(|e| EngineError::OsApiError(format!("Failed to set max protocol version: {}", e)))?;
        builder.set_grease_enabled(true);

        // Set TLS ALPN negotiation for ["h2", "http/1.1"]
        let alpn_protos = b"\x02h2\x08http/1.1";
        builder.set_alpn_protos(alpn_protos)
            .map_err(|e| EngineError::OsApiError(format!("Failed to set ALPN protocols: {}", e)))?;

        // Optimize options for zero-copy efficiency and security
        builder.set_options(SslOptions::NO_COMPRESSION | SslOptions::NO_RENEGOTIATION);

        Ok(Self { connector: builder.build() })
    }

    pub fn get_connector(&self) -> SslConnector {
        self.connector.clone()
    }
}