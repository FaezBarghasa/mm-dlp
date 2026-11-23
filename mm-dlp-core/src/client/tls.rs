use boring::ssl::{SslConnector, SslMethod, SslOptions, SslVersion};
use crate::client::h2::BrowserProfile;
use crate::client::EngineError;

pub struct TlsImpersonator {
    connector: SslConnector,
    profile: BrowserProfile,
}

impl TlsImpersonator {
    pub fn new() -> Result<Self, EngineError> {
        Self::for_browser(BrowserProfile::Chrome)
    }

    pub fn for_browser(profile: BrowserProfile) -> Result<Self, EngineError> {
        let mut builder = SslConnector::builder(SslMethod::tls())
            .map_err(|e| EngineError::OsApiError(format!("Failed to create SSL builder: {}", e)))?;

        match profile {
            BrowserProfile::Chrome => {
                let cipher_list = "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305";
                builder.set_cipher_list(cipher_list)
                    .map_err(|e| EngineError::OsApiError(format!("Failed to set cipher list: {}", e)))?;
                builder.set_grease_enabled(true);
            }
            BrowserProfile::Firefox => {
                let cipher_list = "TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_256_GCM_SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305";
                builder.set_cipher_list(cipher_list)
                    .map_err(|e| EngineError::OsApiError(format!("Failed to set cipher list: {}", e)))?;
                builder.set_grease_enabled(false);
            }
        }

        builder.set_min_proto_version(Some(SslVersion::TLS1_3))
            .map_err(|e| EngineError::OsApiError(format!("Failed to set min protocol version: {}", e)))?;
        builder.set_max_proto_version(Some(SslVersion::TLS1_3))
            .map_err(|e| EngineError::OsApiError(format!("Failed to set max protocol version: {}", e)))?;

        let alpn_protos = b"\x02h2\x08http/1.1";
        builder.set_alpn_protos(alpn_protos)
            .map_err(|e| EngineError::OsApiError(format!("Failed to set ALPN protocols: {}", e)))?;

        builder.set_options(SslOptions::NO_COMPRESSION | SslOptions::NO_RENEGOTIATION);

        Ok(Self {
            connector: builder.build(),
            profile,
        })
    }

    pub fn new_firefox() -> Result<Self, EngineError> {
        Self::for_browser(BrowserProfile::Firefox)
    }

    pub fn profile(&self) -> BrowserProfile {
        self.profile
    }

    pub fn get_connector(&self) -> SslConnector {
        self.connector.clone()
    }
}