# Cross-Platform Optimization & Architecture Plan: Linux, Windows, macOS, and iOS

## Goal Description
Transform `mm-dlp` into an ultra-low latency, maximum-throughput media download engine natively optimized for **Linux, Windows, macOS, and iOS**.

### Key Cross-Platform Subsystems:
1. **Linux**: `io_uring` / `epoll` socket tuning, `TCP_NODELAY`, `SO_RCVBUF` auto-tuning, `posix_fadvise`, and eBPF socket drop monitoring.
2. **Windows**: High-concurrency IOCP via Tokio, `FILE_FLAG_SEQUENTIAL_SCAN`, NTFS block pre-allocation, and DPAPI cookie decryption.
3. **macOS**: `kqueue` reactor, `fcntl(F_RDAHEAD)`, macOS Keychain Services integration, and APFS clonefile integration.
4. **iOS**: Strict Jetsam memory budget ($\le 35\,\text{MB}$ RSS), sandboxed storage (`NSCachesDirectory`), cellular/Wi-Fi adaptive pacing, and UniFFI XCFramework packaging for Swift.

---

## Detailed Phase Breakdown

### Phase 1: OS-Targeted File I/O & Storage Pacing
- **Linux**: `libc::posix_fadvise(fd, 0, 0, POSIX_FADV_SEQUENTIAL)` and `fallocate` pre-allocation.
- **Windows**: `FILE_FLAG_SEQUENTIAL_SCAN` through Tokio `OpenOptionsExt` and `SetEndOfFile` pre-allocation.
- **macOS**: `fcntl(fd, F_RDAHEAD, 1)` and APFS optimization.
- **iOS**: Strictly bounded sliding window buffers ($\le 35\,\text{MB}$ RSS limit) to avoid Jetsam memory kills.

### Phase 2: Platform Socket Tuning & Network Transport Optimization
- Socket options (`TCP_NODELAY`, 4MB `SO_RCVBUF` tuning on broadband).
- iOS cellular sensing (adaptive concurrency scaling 16 -> 4 on cellular links).
- Native TLS verification across platforms via `rustls-platform-verifier` on Apple platforms.

### Phase 3: Platform Security & Browser Cookie Decryption
- **Linux**: Secret Service API + PBKDF2 Chrome cookie decryption.
- **Windows**: Windows DPAPI (`CryptUnprotectData`) + AES-256-GCM.
- **macOS**: Keychain Services ("Chrome Safe Storage") + Safari binary cookies parser.
- **iOS**: Injected Netscape / WKWebView cookie jar via UniFFI.

### Phase 4: Apple Silicon, iOS Framework & UniFFI Swift Packaging
- Compilation targets: `x86_64-apple-darwin`, `aarch64-apple-darwin`, `aarch64-apple-ios`, `aarch64-apple-ios-sim`.
- Universal `MmDlpCore.xcframework` generation script and `Package.swift` SPM bundle.

### Phase 5: eBPF Socket Profiling & Diagnostic Stack (Linux Engine)
- Kernel tracepoint monitoring (`tracepoint:skb:kfree_skb`, `kprobe:tcp_retransmit_skb`) via `bpftrace`.
- Optional `tokio-console` task and waker instrumentation.

### Phase 6: Cross-Platform Verification & CI Build Matrix
- Multi-target compilation tests (`windows-gnu`, `apple-darwin`, `apple-ios`, `apple-ios-sim`).
