# mm-dlp (MultiMedia Downloader)

**mm-dlp** is a fast, safe, and highly extensible media metadata extractor and downloading tool built in Rust. Designed with modularity and concurrency in mind, it reliably parses URLs from the web's most popular media platforms to extract clean, standardized metadata required for downstream processing and downloading.

## Core Vision

The goal of `mm-dlp` is to provide a unified, unified interface for interacting with arbitrary media platforms. Instead of hardcoding URL parsers, the project relies on a dynamic `PlatformRegistry` and a standardized `PlatformExtractor` trait. This allows the tool to scale infinitely as new media sources emerge, while keeping the core logic safe, clean, and thread-safe (`Send + Sync`).

## Features

- **Broad Platform Support**: Out-of-the-box support for 9 major platforms (Video, Audio, Social Posts, and Streams).
- **Extensible Architecture**: Adding a new platform is as simple as implementing a single trait and adding it to the registry.
- **Robust URL Sanitization**: Automatically strips query parameters, tracking tags, fragment identifiers, and trailing slashes to isolate the exact media ID.
- **Concurrency Ready**: Built with Rust's thread-safety guarantees, allowing massive parallel processing of URLs.

## Supported Platforms

The `mm-dlp-core` library currently supports extracting metadata from the following platforms:

| Platform    | Supported Media Types                       |
|-------------|---------------------------------------------|
| **YouTube** | Video, Short                                |
| **Twitter/X**| Post                                       |
| **Instagram**| Post, Reel, IGTV                           |
| **TikTok**  | Video                                       |
| **Reddit**  | Post                                        |
| **Twitch**  | Video (VOD), Clip, Live Stream              |
| **Spotify** | Track, Album, Playlist, Episode             |
| **Vimeo**   | Video                                       |
| **SoundCloud**| Track                                     |

## Installation

Add `mm-dlp-core` to your `Cargo.toml` dependencies:

```toml
[dependencies]
mm-dlp-core = { path = "./mm-dlp-core" }
```

## Usage

The primary entry point for parsing URLs is the `PlatformRegistry`. It automatically routes your URL to the correct extractor.

```rust
use mm_dlp_core::platforms::PlatformRegistry;

fn main() {
    // Initialize the registry with all default platform extractors
    let registry = PlatformRegistry::new();
    
    // Example 1: YouTube Video
    let yt_url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=youtu.be";
    match registry.extract(yt_url) {
        Ok(metadata) => {
            println!("Platform: {}", metadata.platform);       // "YouTube"
            println!("Media Type: {}", metadata.media_type);   // "Video"
            println!("Media ID: {}", metadata.media_id);       // "dQw4w9WgXcQ"
        }
        Err(e) => println!("Error: {}", e),
    }

    // Example 2: Spotify Track
    let spotify_url = "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=123456";
    if let Ok(metadata) = registry.extract(spotify_url) {
        println!("Found {} {} with ID: {}", metadata.platform, metadata.media_type, metadata.media_id);
    }
}
```

## Architecture

### The `MediaMetadata` Struct
Whenever a URL is successfully parsed, the extractor returns a `MediaMetadata` object containing:
- `platform`: The canonical name of the service (e.g., "Twitch").
- `media_type`: The category of the content (e.g., "Clip", "Video", "Post").
- `media_id`: The raw, sanitized unique identifier used by the platform's API.

### The `PlatformExtractor` Trait
All extractors implement the following trait. This guarantees that `mm-dlp` can handle any platform uniformly.

```rust
pub trait PlatformExtractor: Send + Sync {
    fn extract(&self, url: &str) -> Option<MediaMetadata>;
}
```

## Contributing: Adding a New Platform

To add support for a new platform, you do not need to alter the core routing logic. Just follow these steps:

1. **Create the Extractor Struct**: Define a new struct for your platform.
2. **Implement `PlatformExtractor`**: Write the parsing logic to find the unique ID in the URL. Use the internal `clean_id` helper to safely remove URL artifacts.
3. **Register It**: Add your new struct to the `PlatformRegistry::new()` initialization vector.

**Example: Adding a generic platform**
```rust
struct CustomExtractor;

impl PlatformExtractor for CustomExtractor {
    fn extract(&self, url: &str) -> Option<MediaMetadata> {
        if url.contains("customplatform.com/media/") {
            let host_idx = url.find("customplatform.com/media/").unwrap();
            let raw_id_part = &url[host_idx + 25..];
            let id = clean_id(raw_id_part);
            
            if !id.is_empty() {
                return Some(MediaMetadata { 
                    platform: "CustomPlatform".to_string(), 
                    media_type: "Video".to_string(), 
                    media_id: id 
                });
            }
        }
        None
    }
}
```

## License

This project is licensed under the MIT License.