# mm-dlp Developer Guide

Welcome to the developer documentation for **mm-dlp**. This guide is intended for contributors who want to understand the internal architecture, add new platform extractors, or improve the core engine.

## Architecture Overview

At its heart, `mm-dlp-core` is a routing and parsing engine. The core architectural philosophy is **composition and trait-based dispatch**.

Instead of writing a massive, monolithic function with endless `if/else` statements for every supported domain, the project is split into discrete **Extractors**. Each platform gets its own extractor that knows exactly how to parse URLs for that specific service.

### Key Components

1. **`PlatformRegistry`**: The central router. It holds a collection of boxed extractors (`Vec<Box<dyn PlatformExtractor>>`). When `registry.extract(url)` is called, it iterates through its registered extractors until one successfully returns `MediaMetadata`.
2. **`PlatformExtractor`**: A thread-safe trait (`Send + Sync`) that requires a single method: `extract(&self, url: &str) -> Option<MediaMetadata>`.
3. **`MediaMetadata`**: The standardized data structure returned upon successful extraction.
4. **`clean_id` Helper**: A utility function provided by the core library to strip tracking tags (e.g., `?si=...`), query parameters, and trailing slashes from raw ID strings.

---

## Concurrency and Thread Safety

Because `mm-dlp` is built to be a high-performance downloader, the `PlatformExtractor` trait enforces `Send + Sync`. 

When implementing new features or extractors:
- **Avoid interior mutability** (like `RefCell` or `Mutex`) within extractors unless absolutely necessary. Extractors should ideally be stateless URL parsers.
- **Do not use thread-local state**, as the registry might be shared across a thread pool (e.g., using `rayon` or `tokio`) to process thousands of URLs concurrently.

---

## Adding a New Platform Extractor

Adding a new platform is straightforward. Let's walk through adding a fully functional extractor for a hypothetical platform called **EchoStream**.

### 1. Create the Extractor File
Create a new file in the `platforms` directory, for example: `src/platforms/echostream.rs`.

### 2. Implement the Trait
Write the extractor logic. Make sure to handle URL edge cases and utilize `clean_id`.

```rust
use crate::core::{MediaMetadata, PlatformExtractor, clean_id};

pub struct EchoStreamExtractor;

impl PlatformExtractor for EchoStreamExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        // Ensure the URL belongs to EchoStream
        if !url.contains("echostream.tv/") {
            return None;
        }

        // Parse Video URLs: echostream.tv/watch/v/12345abcd
        if let Some(watch_idx) = url.find("/watch/v/") {
            let raw_id = &url[watch_idx + 9..];
            let id = clean_id(raw_id);
            
            if !id.is_empty() {
                return Some(MediaMetadata {
                    platform: "EchoStream".to_string(),
                    media_type: "Video".to_string(),
                    media_id: id,
                });
            }
        }

        // Parse Live Stream URLs: echostream.tv/live/channelname
        if let Some(live_idx) = url.find("/live/") {
            let raw_id = &url[live_idx + 6..];
            let id = clean_id(raw_id);
            
            if !id.is_empty() {
                return Some(MediaMetadata {
                    platform: "EchoStream".to_string(),
                    media_type: "Live Stream".to_string(),
                    media_id: id,
                });
            }
        }

        None
    }
}
```

### 3. Register the Extractor
Open the file where `PlatformRegistry` is defined (likely `src/platforms/mod.rs` or `registry.rs`). 
Add your new extractor to the initialization vector.

```rust
// Inside src/platforms/registry.rs (or similar)

use crate::platforms::echostream::EchoStreamExtractor;
// ... other imports ...

impl PlatformRegistry {
    pub fn new() -> Self {
        let mut extractors: Vec<Box<dyn PlatformExtractor>> = Vec::new();
        
        // Add existing extractors
        extractors.push(Box::new(YouTubeExtractor));
        extractors.push(Box::new(SpotifyExtractor));
        
        // Register your new extractor here
        extractors.push(Box::new(EchoStreamExtractor));
        
        PlatformRegistry { extractors }
    }
}
```

---

## Testing Your Extractor

We strictly enforce unit testing for every platform. When adding an extractor, you must provide tests that cover standard URLs, URLs with query parameters, and invalid URLs.

Append this standard test module to the bottom of your `echostream.rs` file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echostream_video_extraction() {
        let extractor = EchoStreamExtractor;
        
        // Standard URL
        let meta = extractor.extract("https://echostream.tv/watch/v/98765xyz").unwrap();
        assert_eq!(meta.platform, "EchoStream");
        assert_eq!(meta.media_type, "Video");
        assert_eq!(meta.media_id, "98765xyz");

        // URL with trailing query parameters
        let meta_dirty = extractor.extract("https://echostream.tv/watch/v/98765xyz?autoplay=1&ref=twitter").unwrap();
        assert_eq!(meta_dirty.media_id, "98765xyz"); // clean_id should have stripped the query
    }

    #[test]
    fn test_echostream_live_extraction() {
        let extractor = EchoStreamExtractor;
        let meta = extractor.extract("https://echostream.tv/live/gaming_channel/").unwrap();
        assert_eq!(meta.media_type, "Live Stream");
        assert_eq!(meta.media_id, "gaming_channel"); // clean_id should have stripped the trailing slash
    }

    #[test]
    fn test_echostream_invalid_url() {
        let extractor = EchoStreamExtractor;
        assert!(extractor.extract("https://echostream.tv/about-us").is_none());
    }
}
```