use crate::downloader::parallel::SequencedChunk;
use crate::error::EngineError;
use std::collections::BinaryHeap;
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;

pub struct SequentialFlusher {
    heap: BinaryHeap<SequencedChunk>,
    next_expected_index: usize,
}

impl SequentialFlusher {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            next_expected_index: 0,
        }
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
        mut self,
        path: P,
        mut rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
        append: bool,
    ) -> Result<u64, EngineError> {
        let file = Self::open_file_optimized(path.as_ref(), append).await?;
        let mut writer = BufWriter::new(file);

        let mut total_bytes = 0u64;

        while self.next_expected_index < total_segments {
            match rx.recv().await {
                Some(Ok(chunk)) => {
                    self.heap.push(chunk);

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
