use boring::ssl::{SslConnector, SslMethod, SslOptions, SslVersion};
use crate::client::EngineError;

pub struct TlsImpersonator {
    connector: SslConnector,
}

impl TlsImpersonator {
    pub fn new() -> Result<Self, EngineError> {
        let mut builder = SslConnector::builder(SslMethod::tls_client())
            .map_err(|e| EngineError::OsApiError(format!("Failed to create SSL builder: {}", e)))?;

        // Configure standard ciphers: TLS 1.3 GREASE, AES_128_GCM_SHA256, AES_256_GCM_SHA384, CHACHA20_POLY1305_SHA256
        let cipher_list = "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256";
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