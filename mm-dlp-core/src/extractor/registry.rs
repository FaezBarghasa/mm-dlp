use once_cell::sync::Lazy;
use std::sync::Arc;

pub use crate::extractor::fallback_bridge::FallbackBridgeExtractor;
pub use crate::extractor::generic::GenericExtractor;
pub use crate::extractor::soundcloud::SoundCloudExtractor;
pub use crate::extractor::traits::AsyncExtractor;
pub use crate::extractor::vimeo::VimeoExtractor;
pub use crate::extractor::youtube::YouTubeExtractor;

// ==========================================
// Static Routing Registry
// ==========================================

pub static NATIVE_EXTRACTORS: Lazy<Vec<Arc<dyn AsyncExtractor>>> = Lazy::new(|| {
    vec![
        Arc::new(YouTubeExtractor::new()),
        Arc::new(VimeoExtractor::new()),
        Arc::new(SoundCloudExtractor::new()),
    ]
});

pub static GENERIC_EXTRACTOR: Lazy<Arc<dyn AsyncExtractor>> =
    Lazy::new(|| Arc::new(GenericExtractor::new()));

pub static FALLBACK_EXTRACTOR: Lazy<Arc<dyn AsyncExtractor>> =
    Lazy::new(|| Arc::new(FallbackBridgeExtractor::new()));

pub fn route_url(url: &str) -> Option<Arc<dyn AsyncExtractor>> {
    if url.trim().is_empty() {
        return None;
    }
    NATIVE_EXTRACTORS
        .iter()
        .find(|&extractor| extractor.matches_url(url))
        .cloned()
}

pub fn route_url_with_fallback(url: &str) -> Option<Arc<dyn AsyncExtractor>> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Prioritize native high-speed Rust extractors
    if let Some(ext) = route_url(trimmed) {
        return Some(ext);
    }

    // 2. Direct media file / HTML5 page extractor
    let generic = GENERIC_EXTRACTOR.clone();
    if generic.matches_url(trimmed) {
        return Some(generic);
    }

    // 3. Fallback bridge for long-tail extractors
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(FALLBACK_EXTRACTOR.clone());
    }

    None
}
