use crate::downloader::parallel::SequencedChunk;
use crate::error::EngineError;
use std::collections::BinaryHeap;
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct SequentialFlusher {
    heap: BinaryHeap<SequencedChunk>,
    next_expected_index: usize,
    max_backlog: usize,
}

impl SequentialFlusher {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            next_expected_index: 0,
            max_backlog: 64,
        }
    }

    pub fn with_max_backlog(mut self, max_backlog: usize) -> Self {
        self.max_backlog = max_backlog.max(4);
        self
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
                    self.heap.push(chunk);

                    if self.heap.len() > self.max_backlog {
                        return Err(EngineError::IoError(format!(
                            "SequentialFlusher backlog exceeded maximum window of {} chunks waiting for index {}",
                            self.max_backlog, self.next_expected_index
                        )));
                    }

                    // Pop directly and flush contiguous runs matching expectations
                    while let Some(top) = self.heap.peek() {
                        if top.index == self.next_expected_index {
                            if let Some(popped) = self.heap.pop() {
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
                            self.heap.push(chunk);

                            if self.heap.len() > self.max_backlog {
                                return Err(EngineError::IoError(format!(
                                    "SequentialFlusher live backlog exceeded maximum window of {} chunks waiting for index {}",
                                    self.max_backlog, self.next_expected_index
                                )));
                            }

                            while let Some(top) = self.heap.peek() {
                                if top.index == self.next_expected_index {
                                    if let Some(popped) = self.heap.pop() {
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
