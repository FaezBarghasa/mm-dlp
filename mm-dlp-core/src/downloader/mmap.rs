use std::fs::File;
use memmap2::MmapMut;
use crate::error::{Result, EngineError};

pub struct MemoryMappedFileWriter {
    mmap: MmapMut,
}

impl MemoryMappedFileWriter {
    pub fn new(path: &str, file_size: u64) -> Result<Self> {
        let file = File::create(path)?;
        file.set_len(file_size)?; // Pre-allocate file bounds

        let mmap = unsafe { MmapMut::map_mut(&file)? };
        Ok(Self { mmap })
    }

    pub fn write_at_offset(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        if offset + data.len() > self.mmap.len() {
            return Err(EngineError::FileSystemError { reason: "Write out of bounds".to_string() });
        }

        // Zero-copy writes directly into mapped storage memory
        self.mmap[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    pub fn flush_mmap(&mut self) -> Result<()> {
        self.mmap.flush()?;
        Ok(())
    }
}
