use once_cell::sync::Lazy;
use std::sync::Arc;

pub use crate::extractor::soundcloud::SoundCloudExtractor;
pub use crate::extractor::traits::AsyncExtractor;
pub use crate::extractor::vimeo::VimeoExtractor;
pub use crate::extractor::youtube::YouTubeExtractor;

// ==========================================
// Static Routing Registry
// ==========================================

pub static EXTRACTOR_REGISTRY: Lazy<Vec<Arc<dyn AsyncExtractor>>> = Lazy::new(|| {
    vec![
        Arc::new(YouTubeExtractor::new()),
        Arc::new(VimeoExtractor::new()),
        Arc::new(SoundCloudExtractor::new()),
    ]
});

pub fn route_url(url: &str) -> Option<Arc<dyn AsyncExtractor>> {
    EXTRACTOR_REGISTRY
        .iter()
        .find(|&extractor| extractor.matches_url(url))
        .cloned()
}
