use bytes::Bytes;
use h2::client::Builder;

pub const CHROME_INITIAL_WINDOW_SIZE: u32 = 6_291_456;
pub const GECKO_INITIAL_WINDOW_SIZE: u32 = 131_072;
pub const GECKO_MAX_FRAME_SIZE: u32 = 16_384;
pub const GECKO_HEADER_TABLE_SIZE: u32 = 65_536;

pub const SAFARI_INITIAL_WINDOW_SIZE: u32 = 4_194_304;
pub const SAFARI_MAX_FRAME_SIZE: u32 = 16_384;
pub const SAFARI_HEADER_TABLE_SIZE: u32 = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserProfile {
    #[default]
    Chrome,
    Firefox, // Gecko
    Safari,  // WebKit
}

pub struct H2Impersonator {
    builder: Builder,
    profile: BrowserProfile,
}

impl H2Impersonator {
    pub fn new() -> Self {
        Self::for_browser(BrowserProfile::Chrome)
    }

    pub fn for_browser(profile: BrowserProfile) -> Self {
        let mut builder = Builder::new();
        match profile {
            BrowserProfile::Chrome => {
                builder.initial_window_size(CHROME_INITIAL_WINDOW_SIZE);
            }
            BrowserProfile::Firefox => {
                builder.initial_window_size(GECKO_INITIAL_WINDOW_SIZE);
                builder.max_frame_size(GECKO_MAX_FRAME_SIZE);
                builder.header_table_size(GECKO_HEADER_TABLE_SIZE);
            }
            BrowserProfile::Safari => {
                builder.initial_window_size(SAFARI_INITIAL_WINDOW_SIZE);
                builder.max_frame_size(SAFARI_MAX_FRAME_SIZE);
                builder.header_table_size(SAFARI_HEADER_TABLE_SIZE);
            }
        }
        Self { builder, profile }
    }

    pub fn new_firefox() -> Self {
        Self::for_browser(BrowserProfile::Firefox)
    }

    pub fn new_safari() -> Self {
        Self::for_browser(BrowserProfile::Safari)
    }

    pub fn get_builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    pub fn profile(&self) -> BrowserProfile {
        self.profile
    }

    /// Formats pseudo-headers according to browser personality.
    /// Chrome order: :method, :authority, :scheme, :path
    /// Firefox/Gecko order: :method, :path, :authority, :scheme
    /// Safari/WebKit order: :method, :scheme, :path, :authority
    pub fn format_pseudo_headers_for(
        profile: BrowserProfile,
        method: &str,
        authority: &str,
        scheme: &str,
        path: &str,
    ) -> Vec<(&'static str, Bytes)> {
        match profile {
            BrowserProfile::Chrome => vec![
                (":method", Bytes::copy_from_slice(method.as_bytes())),
                (":authority", Bytes::copy_from_slice(authority.as_bytes())),
                (":scheme", Bytes::copy_from_slice(scheme.as_bytes())),
                (":path", Bytes::copy_from_slice(path.as_bytes())),
            ],
            BrowserProfile::Firefox => vec![
                (":method", Bytes::copy_from_slice(method.as_bytes())),
                (":path", Bytes::copy_from_slice(path.as_bytes())),
                (":authority", Bytes::copy_from_slice(authority.as_bytes())),
                (":scheme", Bytes::copy_from_slice(scheme.as_bytes())),
            ],
            BrowserProfile::Safari => vec![
                (":method", Bytes::copy_from_slice(method.as_bytes())),
                (":scheme", Bytes::copy_from_slice(scheme.as_bytes())),
                (":path", Bytes::copy_from_slice(path.as_bytes())),
                (":authority", Bytes::copy_from_slice(authority.as_bytes())),
            ],
        }
    }

    /// Enforces absolute chrome-matched pseudo-header ordering inside outgoing request headers (backward compatible).
    pub fn format_pseudo_headers(
        method: &str,
        authority: &str,
        scheme: &str,
        path: &str,
    ) -> Vec<(&'static str, Bytes)> {
        Self::format_pseudo_headers_for(BrowserProfile::Chrome, method, authority, scheme, path)
    }
}

impl Default for H2Impersonator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gecko_pseudo_headers_order() {
        let headers = H2Impersonator::format_pseudo_headers_for(
            BrowserProfile::Firefox,
            "GET",
            "example.com",
            "https",
            "/video",
        );
        assert_eq!(headers[0].0, ":method");
        assert_eq!(headers[1].0, ":path");
        assert_eq!(headers[2].0, ":authority");
        assert_eq!(headers[3].0, ":scheme");
    }

    #[test]
    fn test_chrome_pseudo_headers_order() {
        let headers =
            H2Impersonator::format_pseudo_headers("GET", "example.com", "https", "/video");
        assert_eq!(headers[0].0, ":method");
        assert_eq!(headers[1].0, ":authority");
        assert_eq!(headers[2].0, ":scheme");
        assert_eq!(headers[3].0, ":path");
    }

    #[test]
    fn test_safari_pseudo_headers_order() {
        let headers = H2Impersonator::format_pseudo_headers_for(
            BrowserProfile::Safari,
            "GET",
            "example.com",
            "https",
            "/video",
        );
        assert_eq!(headers[0].0, ":method");
        assert_eq!(headers[1].0, ":scheme");
        assert_eq!(headers[2].0, ":path");
        assert_eq!(headers[3].0, ":authority");
    }
}
