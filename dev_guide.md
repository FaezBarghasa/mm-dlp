# mm-dlp Developer Guide

Welcome, contributor! This guide provides a technical overview of the `mm-dlp` project, its architecture, and the development workflow. It is intended to help you get up to speed quickly and contribute effectively.

## 1. Project Structure

`mm-dlp` is organized as a Cargo workspace with two primary crates:

-   `mm-dlp-cli`: A lightweight command-line interface for the core engine. This crate is responsible for parsing user arguments (`clap`) and initializing the `tokio` async runtime.
-   `mm-dlp-core`: The heart of the project. This library contains all the core logic for networking, extraction, downloading, and post-processing. It is designed to be fully embeddable in other applications via a C-FFI layer.

### Core Crate Layout (`mm-dlp-core/src`)

The core engine is divided into several modules, each with a distinct responsibility:

```
mm-dlp-core/src
├── client/          # Network engine (TLS/HTTP2 impersonation, cookie store)
├── downloader/      # Parallel segment downloader, file flusher, and mmap support
├── error.rs         # Unified error type for the entire engine
├── extractor/       # Site extractor traits and the central registry
├── js/              # Embedded JavaScript engine for signature deciphering
├── plugin/          # WebAssembly (WASM) plugin host and sandbox
├── postprocessor/   # Media muxing and transcoding (e.g., FFmpeg)
├── uniffi/          # UniFFI definition file (`mm-dlp.udl`) for FFI bindings
└── lib.rs           # Main library entry point, modules, and FFI scaffolding
```

-   **`client`**: Handles all outgoing network requests. Its primary job is to impersonate browsers by customizing TLS handshakes and HTTP/2 frames to bypass WAFs.
-   **`extractor`**: Defines the `AsyncExtractor` trait, the common interface for all site-specific parsers. It also contains the registry that maps URLs to the correct extractor.
-   **`downloader`**: Manages the high-performance download pipeline. It takes a list of media segments, fetches them concurrently, and writes them to disk in the correct order.
-   **`js`**: Contains the sandboxed QuickJS runtime. This is used to execute obfuscated JavaScript from websites (e.g., to decipher YouTube's signature).
-   **`plugin`**: The host environment for running extractors compiled to WebAssembly. It provides a secure sandbox with limited access to system resources.
-   **`postprocessor`**: Handles tasks that run after the download is complete, such as merging separate video and audio files using FFmpeg.
-   **`error.rs`**: Defines the `EngineError` enum, which provides a unified error-handling mechanism across the entire library and FFI boundary.
-   **`uniffi/mm-dlp.udl`**: The **UniFFI Definition Language** file. This contract defines the types and functions that are exposed across the FFI boundary to other languages like Kotlin, Swift, or Python.

## 2. Core Concepts

### The Async Runtime (`tokio`)

`mm-dlp-core` is async-first. All I/O operations (network requests, disk writes) are non-blocking and managed by the `tokio` runtime. This allows us to achieve massive concurrency with a small number of OS threads, which is critical for performance. When writing code, always prefer `async/.await` and use the async-compatible versions of standard library types (e.g., `tokio::fs` instead of `std::fs`).

### The FFI Boundary (`uniffi`)

We use `uniffi` to automatically generate the boilerplate for our Foreign Function Interface. The single source of truth for our public API is `mm-dlp.udl`.

-   **How it works:** The `build.rs` script in `mm-dlp-core` invokes the `uniffi` code generator. It parses the `.udl` file and generates Rust scaffolding (`uniffi::include_scaffolding!`) as well as the corresponding interface definitions for foreign languages (e.g., `.kt`, `.swift`).
-   **Changing the API:** To add or modify a function or type that is exposed to the outside world, you **must** update the `.udl` file first. Then, run `cargo build` to regenerate the scaffolding and implement the new logic in Rust.

### Error Handling

All fallible functions in the core library should return a `crate::error::Result<T>`, which is an alias for `std::result::Result<T, EngineError>`. The `EngineError` enum is designed to be FFI-safe.

Use the `?` operator to propagate errors upwards. If you are handling an error from an external library (like `reqwest` or `std::io`), use the `.into()` method to convert it into our `EngineError` type, as `From` implementations are provided.

```rust
// Example of error handling
use crate::error::{Result, EngineError};
use std::fs;

fn do_something() -> Result<()> {
    // This will automatically convert std::io::Error into EngineError::FileSystemError
    let content = fs::read_to_string("foo.txt")?;

    if content.is_empty() {
        // Return a custom error
        return Err(EngineError::InternalPanic { reason: "File is empty".to_string() });
    }

    Ok(())
}
```

## 3. How-To Guides

### Adding a New Native Extractor

1.  **Create the File:** Create a new file for your extractor in `mm-dlp-core/src/extractor/`, e.g., `my_platform.rs`.
2.  **Implement the Trait:** Implement the `AsyncExtractor` trait for your new struct.

    ```rust
    // mm-dlp-core/src/extractor/my_platform.rs
    use async_trait::async_trait;
    use crate::error::Result;
    use crate::extractor::traits::{AsyncExtractor, MediaInfo};

    pub struct MyPlatformExtractor;

    #[async_trait]
    impl AsyncExtractor for MyPlatformExtractor {
        fn matches_url(&self, url: &str) -> bool {
            // Use a regex or simple string matching
            url.contains("my-platform.com/video/")
        }

        async fn extract_metadata(&self, client: &reqwest::Client, url: &str) -> Result<MediaInfo> {
            // 1. Fetch the webpage content using the provided client
            // 2. Parse the HTML/JSON to find media information
            // 3. Construct and return a MediaInfo struct
            todo!();
        }
    }
    ```

3.  **Register the Extractor:** Add your new extractor to the central registry in `mm-dlp-core/src/extractor/registry.rs`. This will allow the engine to automatically use it for matching URLs.

### Building and Testing

-   **Build the project:**
    ```bash
    cargo build
    ```
-   **Run all tests:**
    ```bash
    cargo test
    ```
-   **Check for compilation errors without building:**
    ```bash
    cargo check
    ```
-   **Run the CLI for manual testing:**
    ```bash
    cargo run --package mm-dlp-cli -- --url "URL_TO_TEST"
    ```

## 4. Contribution Guidelines

1.  **Code Style:** All code must be formatted with `rustfmt`. You can run `cargo fmt` before committing to ensure your code meets the style guidelines.
2.  **Clippy:** Use `cargo clippy` to catch common mistakes and improve code quality.
3.  **Commit Messages:** Write clear and descriptive commit messages.
4.  **Pull Requests:**
    -   Create a new branch for your feature or bugfix.
    -   Ensure all tests pass before submitting.
    -   Keep pull requests focused on a single issue.

Thank you for contributing to `mm-dlp`!
