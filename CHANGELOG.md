# Changelog

All notable changes to the `mm-dlp` project will be documented in this file.

## [Unreleased]

### Added
- **Core Engine**: Initialized the `mm-dlp-core` crate with foundational data structures, traits, and error types.
- **Extractors**: Added fully functional platform extractors for:
  - **YouTube**: Stream provider via HTTP scraping.
  - **SoundCloud**: Stream provider via HTTP scraping.
  - **Spotify**: Metadata extraction support.
- **Orchestration**: Built resolver, downloader, and audio processor pipelines.
- **UniFFI Bridge**: Configured `uniffi.toml` and scaffolding for Kotlin/Swift FFI generation (`mm-dlp.udl`).
- **Build Infrastructure**: Added shell script (`scripts/generate_bindings.sh`) to automate Android cross-compilation (`aarch64-linux-android`, `x86_64-linux-android`) and Kotlin bindings generation.

### Changed
- **Dependencies**: Removed heavy system-dependent native crates (`ffmpeg-sys-next`, `boring-sys`) from `mm-dlp-core/Cargo.toml` to fix Clang build-script panics on missing `libavutil.pc` during cross-compilation. This unblocked the `cargo check` and `uniffi-bindgen` processes.
- **Build Process**: Modified `.cargo/config.toml` to ensure the correct linker bindings and `bindgen` clang layout arguments are passed during Android targeting.
