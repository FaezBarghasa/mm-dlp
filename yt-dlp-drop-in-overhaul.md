# Architectural Overhaul & Phased Implementation Plan: `mm-dlp` Drop-in Parity Engine

## Executive Summary & Engineering Objective

This engineering plan outlines the structural overhaul of `mm-dlp` to fulfill its mandate as a **high-throughput, zero-panic, drop-in replacement for `yt-dlp`**. 

Following the technical audit (`/brutal-honesty-review`), this plan addresses all 8 identified vulnerabilities and architectural flaws (F1 through F8), integrating advanced Rust async internals (`tokio`, backpressure semaphores, lock-free flusher channels), production-grade JavaScript sandbox deciphering (`rquickjs`), container-aware media muxing, and socket performance tuning to outperform Python's single-threaded GIL architecture while maintaining 100% command-line and schema parity.

---

## Detailed Phase Breakdown

### Phase 1: Bounded-Memory Ring Buffer & Streaming Flusher Pipeline (Fix F1)
**Goal**: Eliminate unbounded memory allocations in `HlsDownloader` and `DashDownloader` by replacing full-stream buffering (`Vec<Option<Bytes>>`) with an async sliding-window flusher bounded to `concurrency * 2` segments (max 32 MB RAM).
- Sliding-Window Backpressure Control via `tokio::sync::Semaphore`.
- Refactor `SequentialFlusher` to stream ordered chunks directly into `tokio::io::BufWriter<tokio::fs::File>`.
- Workers send downloaded `SequencedChunk` instances over a bounded `mpsc::channel(concurrency * 2)`.

### Phase 2: Robust MPEG-DASH Manifest Engine & Timeline Parsing (Fix F2)
**Goal**: Overhaul the DASH parser to accurately compute total segment counts and expand template URLs for arbitrary duration streams, multi-period VoDs, and `<SegmentTimeline>` manifests with repetition counters (`r`).
- Parse root `mediaPresentationDuration="PT1H23M45.67S"` into seconds.
- Compute segment count from `@timescale` and `@duration`.
- Parse `<SegmentTimeline>` with `<S t="..." d="..." r="..."/>` handling `r` repetitions.
- Support `$Number$`, `$Number%0Nd$`, `$Time$`, and `$RepresentationID$` identifier substitution.

### Phase 3: Production YouTube Player JS & QuickJS Decipher Wiring (Fix F3)
**Goal**: Wire live JavaScript deciphering into `YouTubeExtractor` so protected streams (`signatureCipher` / `cipher`) and throttled streams (`n` parameter) are resolved automatically.
- Fetch and cache base player JS (`base.js`) in `Arc<RwLock<Option<String>>>`.
- Extract signature decipher function and helper operations via regex AST analysis.
- Execute cipher deciphering in `tokio::task::spawn_blocking` using `SandboxJsEngine` (`rquickjs`).
- Solve and transform `n`-parameter challenge to bypass YouTube download throttling.

### Phase 4: Fork-Bomb Elimination & Container-Aware FFmpeg Muxer (Fix F4 & F5)
**Goal**: Guarantee process safety by preventing recursive execution of `mm-dlp` in `FallbackBridgeExtractor`, and ensure FFmpeg muxing produces valid containers without illegal codec errors.
- Check `std::env::current_exe()`: `which::which("yt-dlp")` must not match running binary.
- Inspect and set `MM_DLP_FALLBACK_ACTIVE=1` environment variable before invoking child processes.
- Container-aware FFmpeg muxing in `processor.rs`:
  - If output is `.webm`: `-c:v copy` and `-c:a libopus` (or `-c:a copy` if input is Opus). Never `-c:a aac`.
  - If output is `.mp4`: `-c:v copy` and `-c:a copy` (if input is AAC) or `-c:a aac`.
  - If output is `.mkv`: `-c:v copy -c:a copy` (lossless stream copy).

### Phase 5: Workspace Zero-Panic Safety & AES Key Memoization (Fix F6, F7, F8)
**Goal**: Eradicate all remaining `.unwrap()` calls in production source code, implement thread-safe AES-128 key caching in `HlsDownloader`, and clean up dead dependencies.
- Replace all 10 production `.unwrap()` calls across `mm-dlp-core` (`processor.rs`, `platforms.rs`, `resolver.rs`, `native_mux.rs`, `client/h3.rs`, `downloader/flusher.rs`, `downloader/parallel.rs`, `extractor/common.rs`).
- Implement `Arc<RwLock<HashMap<String, Bytes>>>` AES-128 key memoization in `HlsDownloader`.
- Clean up / feature gate `ffmpeg-next` and dead code.

### Phase 6: High-Performance Network & Socket Profiling (Rust Async Internals)
**Goal**: Tune network sockets, connection reuse, and TCP buffers to minimize packet drops and maximize throughput.
- Set `tcp_nodelay(true)`, connection pool idle timeout (90s), and max idle connections (32).
- HTTP/2 adaptive window sizing and keepalive pings.
- Tracing instrumentation and eBPF socket drop profiling guide.

### Phase 7: Drop-in CLI Parity Validation Suite & Final Verification
**Goal**: Validate full behavioral and flag parity between `mm-dlp` and `yt-dlp` across standard CLI workflows.
- Verification commands:
  - `cargo fmt --check`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
- CLI parity tests with mock and live streaming URLs, JSON schema conformance (`-j`), format selector tests (`-f`), torrent engine tests.
