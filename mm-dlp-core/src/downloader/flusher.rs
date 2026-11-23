use crate::downloader::parallel::SequencedChunk;
use crate::error::EngineError;
use std::collections::BinaryHeap;
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Returns the detected system physical RAM in bytes.
pub fn get_system_ram_bytes() -> usize {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut info = std::mem::zeroed::<libc::sysinfo>();
        if libc::sysinfo(&mut info) == 0 {
            let total = (info.totalram as u64) * (info.mem_unit as u64);
            (total as usize).max(512 * 1024 * 1024)
        } else {
            512 * 1024 * 1024
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut mib = [libc::CTL_HW, libc::HW_MEMSIZE];
        let mut mem_size: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        unsafe {
            if libc::sysctl(
                mib.as_mut_ptr(),
                2,
                &mut mem_size as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            ) == 0 {
                (mem_size as usize).max(512 * 1024 * 1024)
            } else {
                512 * 1024 * 1024
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        unsafe {
            let mut mem_status = std::mem::zeroed::<MEMORYSTATUSEX>();
            mem_status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut mem_status) != 0 {
                (mem_status.ullTotalPhys as usize).max(512 * 1024 * 1024)
            } else {
                512 * 1024 * 1024
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            100 * 1024 * 1024
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            512 * 1024 * 1024
        }
    }
}

/// Returns the platform default streaming buffer size:
/// - Mobile OS (iOS, Android): at least 100 MiB
/// - Desktop OS (Linux, macOS, Windows): 512 MiB
pub fn default_streaming_buffer_size() -> usize {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        100 * 1024 * 1024 // 100 MiB on mobile
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        512 * 1024 * 1024 // 512 MiB on desktop OS
    }
}

/// Parses a human-readable or unit-based buffer size string (e.g. "100M", "512M", "2G", "max").
pub fn parse_buffer_size(s: &str) -> Result<usize, String> {
    let s = s.trim().to_lowercase();
    if s == "default" || s.is_empty() {
        return Ok(default_streaming_buffer_size());
    }
    if s == "max" {
        return Ok(get_system_ram_bytes());
    }
    if let Ok(bytes) = s.parse::<usize>() {
        return Ok(bytes);
    }
    if let Some(num) = s.strip_suffix("gb").or_else(|| s.strip_suffix('g')) {
        let val: f64 = num.parse().map_err(|e| format!("Invalid GB value: {}", e))?;
        return Ok((val * 1024.0 * 1024.0 * 1024.0) as usize);
    }
    if let Some(num) = s.strip_suffix("mb").or_else(|| s.strip_suffix('m')) {
        let val: f64 = num.parse().map_err(|e| format!("Invalid MB value: {}", e))?;
        return Ok((val * 1024.0 * 1024.0) as usize);
    }
    if let Some(num) = s.strip_suffix("kb").or_else(|| s.strip_suffix('k')) {
        let val: f64 = num.parse().map_err(|e| format!("Invalid KB value: {}", e))?;
        return Ok((val * 1024.0) as usize);
    }
    Err(format!("Unrecognized buffer size format: {}", s))
}

pub struct SequentialFlusher {
    heap: BinaryHeap<SequencedChunk>,
    next_expected_index: usize,
    max_backlog: usize,
    current_buffered_bytes: usize,
    max_buffer_bytes: usize,
}

impl SequentialFlusher {
    pub fn new() -> Self {
        let max_buffer = default_streaming_buffer_size();
        let max_backlog = (max_buffer / (64 * 1024)).max(64);
        Self {
            heap: BinaryHeap::new(),
            next_expected_index: 0,
            max_backlog,
            current_buffered_bytes: 0,
            max_buffer_bytes: max_buffer,
        }
    }

    pub fn with_max_backlog(mut self, max_backlog: usize) -> Self {
        self.max_backlog = max_backlog.max(4);
        self
    }

    pub fn with_max_buffer_bytes(mut self, max_buffer_bytes: usize) -> Self {
        self.max_buffer_bytes = max_buffer_bytes.max(1024 * 1024);
        self.max_backlog = (self.max_buffer_bytes / (64 * 1024)).max(64);
        self
    }

    pub fn max_buffer_bytes(&self) -> usize {
        self.max_buffer_bytes
    }

    /// Flush sequenced chunks to any asynchronous writer.
    pub async fn flush_to_writer<W: AsyncWrite + Unpin + Send>(
        mut self,
        mut writer: W,
        mut rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
    ) -> Result<u64, EngineError> {
        let mut total_bytes = 0u64;

        while self.next_expected_index < total_segments {
            match rx.recv().await {
                Some(Ok(chunk)) => {
                    self.current_buffered_bytes += chunk.data.len();
                    self.heap.push(chunk);

                    if self.heap.len() > self.max_backlog
                        || self.current_buffered_bytes > self.max_buffer_bytes
                    {
                        return Err(EngineError::IoError(format!(
                            "SequentialFlusher backlog exceeded limits (chunks: {}/{}, bytes: {}/{} bytes) waiting for index {}",
                            self.heap.len(),
                            self.max_backlog,
                            self.current_buffered_bytes,
                            self.max_buffer_bytes,
                            self.next_expected_index
                        )));
                    }

                    // Pop directly and flush contiguous runs matching expectations
                    while let Some(top) = self.heap.peek() {
                        if top.index == self.next_expected_index {
                            if let Some(popped) = self.heap.pop() {
                                self.current_buffered_bytes =
                                    self.current_buffered_bytes.saturating_sub(popped.data.len());
                                total_bytes += popped.data.len() as u64;
                                writer
                                    .write_all(&popped.data)
                                    .await
                                    .map_err(|e| EngineError::IoError(e.to_string()))?;
                                self.next_expected_index += 1;
                            }
                        } else {
                            break;
                        }
                    }
                }
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }

        writer
            .flush()
            .await
            .map_err(|e| EngineError::IoError(e.to_string()))?;

        if self.next_expected_index < total_segments {
            Err(EngineError::IoError(
                "Channel closed before all segments were received".into(),
            ))
        } else {
            Ok(total_bytes)
        }
    }

    /// Stream sequenced chunks directly to standard output without touching disk.
    pub async fn flush_to_stdout(
        self,
        rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
    ) -> Result<u64, EngineError> {
        let stdout = tokio::io::stdout();
        self.flush_to_writer(stdout, rx, total_segments).await
    }

    /// Flush continuous live stream chunks to an asynchronous writer until cancelled or channel closed.
    pub async fn flush_live_to_writer<W: AsyncWrite + Unpin + Send>(
        mut self,
        mut writer: W,
        mut rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        cancel_token: CancellationToken,
    ) -> Result<u64, EngineError> {
        let mut total_bytes = 0u64;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    break;
                }
                chunk_res = rx.recv() => {
                    match chunk_res {
                        Some(Ok(chunk)) => {
                            self.current_buffered_bytes += chunk.data.len();
                            self.heap.push(chunk);

                            if self.heap.len() > self.max_backlog
                                || self.current_buffered_bytes > self.max_buffer_bytes
                            {
                                return Err(EngineError::IoError(format!(
                                    "SequentialFlusher live backlog exceeded limits (chunks: {}/{}, bytes: {}/{} bytes) waiting for index {}",
                                    self.heap.len(),
                                    self.max_backlog,
                                    self.current_buffered_bytes,
                                    self.max_buffer_bytes,
                                    self.next_expected_index
                                )));
                            }

                            while let Some(top) = self.heap.peek() {
                                if top.index == self.next_expected_index {
                                    if let Some(popped) = self.heap.pop() {
                                        self.current_buffered_bytes =
                                            self.current_buffered_bytes.saturating_sub(popped.data.len());
                                        total_bytes += popped.data.len() as u64;
                                        writer
                                            .write_all(&popped.data)
                                            .await
                                            .map_err(|e| EngineError::IoError(e.to_string()))?;
                                        self.next_expected_index += 1;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => return Err(e),
                        None => break,
                    }
                }
            }
        }

        // Drain any contiguous segments remaining in heap
        while let Some(top) = self.heap.peek() {
            if top.index == self.next_expected_index {
                if let Some(popped) = self.heap.pop() {
                    total_bytes += popped.data.len() as u64;
                    let _ = writer.write_all(&popped.data).await;
                    self.next_expected_index += 1;
                }
            } else {
                break;
            }
        }

        let _ = writer.flush().await;
        Ok(total_bytes)
    }

    /// Stream continuous live stream chunks directly to stdout.
    pub async fn flush_live_to_stdout(
        self,
        rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        cancel_token: CancellationToken,
    ) -> Result<u64, EngineError> {
        let stdout = tokio::io::stdout();
        self.flush_live_to_writer(stdout, rx, cancel_token).await
    }

    pub async fn flush_to_disk<P: AsRef<Path>>(
        self,
        path: P,
        rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
    ) -> Result<u64, EngineError> {
        self.flush_to_disk_with_append(path, rx, total_segments, false)
            .await
    }

    pub async fn flush_to_disk_with_append<P: AsRef<Path>>(
        self,
        path: P,
        rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
        append: bool,
    ) -> Result<u64, EngineError> {
        let file = Self::open_file_optimized(path.as_ref(), append).await?;
        let writer = BufWriter::new(file);
        self.flush_to_writer(writer, rx, total_segments).await
    }

    /// Open file with platform-specific sequential scan hints and flags.
    pub async fn open_file_optimized(path: &Path, append: bool) -> Result<File, EngineError> {
        let mut opts = OpenOptions::new();
        opts.create(true);
        if append {
            opts.append(true);
        } else {
            opts.write(true).truncate(true);
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.custom_flags(0x08000000); // FILE_FLAG_SEQUENTIAL_SCAN
        }

        let file = opts
            .open(path)
            .await
            .map_err(|e| EngineError::IoError(e.to_string()))?;

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL);
            }
        }

        #[cfg(target_os = "macos")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            unsafe {
                libc::fcntl(fd, libc::F_RDAHEAD, 1);
            }
        }

        Ok(file)
    }

    /// Preallocate disk space on platforms that support it to prevent file fragmentation.
    pub async fn preallocate_file(file: &File, total_bytes: u64) -> Result<(), EngineError> {
        if total_bytes == 0 {
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            let ret = unsafe { libc::fallocate(fd, 0, 0, total_bytes as libc::off_t) };
            if ret == 0 {
                return Ok(());
            }
        }

        file.set_len(total_bytes)
            .await
            .map_err(|e| EngineError::IoError(format!("Failed to preallocate file: {}", e)))
    }
}

impl Default for SequentialFlusher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_sequential_flush_to_writer_reordering() {
        let flusher = SequentialFlusher::new();
        let (tx, rx) = mpsc::channel(10);

        let chunks = vec![
            SequencedChunk {
                index: 2,
                data: Bytes::from_static(b"chunk2"),
            },
            SequencedChunk {
                index: 0,
                data: Bytes::from_static(b"chunk0"),
            },
            SequencedChunk {
                index: 1,
                data: Bytes::from_static(b"chunk1"),
            },
        ];

        for chunk in chunks {
            tx.send(Ok(chunk)).await.unwrap();
        }

        let mut output = Cursor::new(Vec::new());
        let total_bytes = flusher.flush_to_writer(&mut output, rx, 3).await.unwrap();

        assert_eq!(total_bytes, 18);
        assert_eq!(output.into_inner(), b"chunk0chunk1chunk2");
    }

    #[tokio::test]
    async fn test_sequential_live_flusher_with_cancellation() {
        let flusher = SequentialFlusher::new();
        let (tx, rx) = mpsc::channel(10);
        let cancel_token = CancellationToken::new();

        tx.send(Ok(SequencedChunk {
            index: 0,
            data: Bytes::from_static(b"live_0"),
        }))
        .await
        .unwrap();

        let cancel_clone = cancel_token.clone();
        let mut output = Cursor::new(Vec::new());
        let flusher_task = tokio::spawn(async move {
            flusher
                .flush_live_to_writer(&mut output, rx, cancel_clone)
                .await
        });

        // Trigger cancellation to stop the live stream
        cancel_token.cancel();
        let res = flusher_task.await.unwrap();
        assert!(res.is_ok());
    }
}
