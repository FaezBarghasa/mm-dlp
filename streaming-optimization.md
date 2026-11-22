# Architecture & Phased Implementation Plan: Video & Audio Streaming Optimization

## Goal Description
Transform `mm-dlp` into a high-performance, real-time streaming engine for video and audio. 
Elevate `mm-dlp` from an offline disk downloader to a zero-disk-spill progressive streaming system with stdout piping (`-o -`), live stream edge tracking (HLS / DASH), concurrent dual-stream on-the-fly muxing, adaptive jitter buffering, and Linux zero-copy socket splicing (`splice(2)`).

---

## Detailed Phase Breakdown

### Phase 1: Real-Time Stdout Pipe Streaming (`-o -`) & Zero-Disk Flusher
- CLI `-o -` / `--stdout` detection and telemetry redirection to `stderr`.
- Generic `AsyncWrite` flusher supporting `tokio::io::Stdout` directly without disk files.
- Zero disk footprint, instantaneous time-to-first-frame for players (`mm-dlp -o - <URL> | mpv -`).

### Phase 2: Live Stream Engine (Dynamic HLS & DASH Live Edge)
- Continuous live HLS streaming (detect absence of `#EXT-X-ENDLIST`).
- Media sequence (`#EXT-X-MEDIA-SEQUENCE`) and target duration polling loop.
- Dynamic MPEG-DASH MPD (`type="dynamic"`) wall-clock segment generation.

### Phase 3: Real-Time Dual-Stream On-The-Fly Muxing Pipe
- Spawns FFmpeg with concurrent input pipes (`-i pipe:3 -i pipe:4`).
- Feeds video and audio chunks concurrently as received, outputting fragmented MP4 or WebM directly to disk or stdout.
- Cuts disk writes by 50% and eliminates all intermediate temporary files.

### Phase 4: Adaptive Jitter Buffering & EWMA Bandwidth Pacing
- Exponentially Weighted Moving Average (EWMA) throughput estimation.
- Adaptive prefetch window maintaining an optimal 10-second healthy jitter buffer.
- Player-paced backpressure (pauses network fetching when downstream player pipe is full).

### Phase 5: Kernel Zero-Copy Socket Splicing (Linux `splice` & eBPF)
- Linux `libc::splice` zero-copy forwarding from socket to stdout pipe.
- Standalone `scripts/trace_streaming_jitter.bt` for kernel drop & latency tracing.

### Phase 6: Streaming Audio DSP & Real-Time Transcoding
- Streaming audio transcoding on the fly straight to stdout without temporary files.
- Optional real-time EBU R128 loudness normalization filter.

### Phase 7: Verification Suite & End-to-End Parity Tests
- Automated unit and integration tests for stdout piping, live sequence polling, and EWMA estimator.
- Verification commands: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`.
