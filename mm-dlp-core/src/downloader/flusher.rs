use crate::downloader::parallel::SequencedChunk;
use crate::error::EngineError;
use std::collections::BinaryHeap;
use std::path::Path;
use tokio::fs::OpenOptions;
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
        self.flush_to_disk_with_append(path, rx, total_segments, false).await
    }

    pub async fn flush_to_disk_with_append<P: AsRef<Path>>(
        mut self,
        path: P,
        mut rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
        append: bool,
    ) -> Result<u64, EngineError> {
        let file = if append {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await
                .map_err(|e| EngineError::IoError(e.to_string()))?
        } else {
            File::create(path)
                .await
                .map_err(|e| EngineError::IoError(e.to_string()))?
        };
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
}

impl Default for SequentialFlusher {
    fn default() -> Self {
        Self::new()
    }
}
