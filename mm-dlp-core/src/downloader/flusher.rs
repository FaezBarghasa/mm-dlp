use crate::client::EngineError;
use crate::downloader::parallel::SequencedChunk;
use std::collections::BinaryHeap;
use std::path::Path;
use tokio::fs::File;
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
        mut self,
        path: P,
        mut rx: mpsc::Receiver<Result<SequencedChunk, EngineError>>,
        total_segments: usize,
    ) -> Result<(), EngineError> {
        let file = File::create(path).await.map_err(|e| EngineError::FileSystemError(e.to_string()))?;
        let mut writer = BufWriter::new(file);

        while self.next_expected_index < total_segments {
            match rx.recv().await {
                Some(Ok(chunk)) => {
                    self.heap.push(chunk);

                    // Pop directly and flush contiguous runs matching expectations
                    while let Some(top) = self.heap.peek() {
                        if top.index == self.next_expected_index {
                            let popped = self.heap.pop().unwrap();
                            writer
                                .write_all(&popped.data)
                                .await
                                .map_err(|e| EngineError::FileSystemError(e.to_string()))?;
                            self.next_expected_index += 1;
                        } else {
                            break;
                        }
                    }
                }
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }

        writer.flush().await.map_err(|e| EngineError::FileSystemError(e.to_string()))?;

        if self.next_expected_index < total_segments {
            Err(EngineError::OsApiError("Channel closed before all segments were received".into()))
        } else {
            Ok(())
        }
    }
}

impl Default for SequentialFlusher {
    fn default() -> Self {
        Self::new()
    }
}