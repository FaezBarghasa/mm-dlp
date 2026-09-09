use bytes::Bytes;
use h2::client::Builder;

pub const CHROME_INITIAL_WINDOW_SIZE: u32 = 6_291_456;

pub struct H2Impersonator {
    builder: Builder,
}

impl H2Impersonator {
    pub fn new() -> Self {
        let mut builder = Builder::new();
        builder.initial_window_size(CHROME_INITIAL_WINDOW_SIZE);
        Self { builder }
    }

    pub fn get_builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    /// Enforces absolute chrome-matched pseudo-header ordering inside outgoing request headers.
    pub fn format_pseudo_headers(method: &str, authority: &str, scheme: &str, path: &str) -> Vec<(&'static str, Bytes)> {
        vec![
            (":method", Bytes::copy_from_slice(method.as_bytes())),
            (":authority", Bytes::copy_from_slice(authority.as_bytes())),
            (":scheme", Bytes::copy_from_slice(scheme.as_bytes())),
            (":path", Bytes::copy_from_slice(path.as_bytes())),
        ]
    }
}

impl Default for H2Impersonator {
    fn default() -> Self {
        Self::new()
    }
}