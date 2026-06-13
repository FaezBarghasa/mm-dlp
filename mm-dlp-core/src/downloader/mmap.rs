use crate::client::EngineError;
use memmap2::MmapMut;
use std::fs::{File, OpenOptions};
use std::path::Path;

pub struct MmapWriter {
    mmap: MmapMut,
    _file: File, // Retain handle to prevent the file from dropping
}

impl MmapWriter {
    pub fn new<P: AsRef<Path>>(path: P, size: u64) -> Result<Self, EngineError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| EngineError::FileSystemError(format!("Failed to open file: {}", e)))?;

        // Pre-allocate blocks on disk securely
        Self::preallocate(&file, size)?;

        let mmap = unsafe { MmapMut::map_mut(&file) }
            .map_err(|e| EngineError::FileSystemError(format!("Failed to map memory: {}", e)))?;

        Ok(Self { mmap, _file: file })
    }

    #[cfg(target_family = "unix")]
    fn preallocate(file: &File, size: u64) -> Result<(), EngineError> {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        
        // Tell the OS to allocate contiguous file system blocks directly
        let ret = unsafe { libc::posix_fallocate(fd, 0, size as libc::off_t) };
        if ret != 0 {
            return Err(EngineError::FileSystemError(format!("posix_fallocate failed with code: {}", ret)));
        }
        Ok(())
    }

    #[cfg(target_family = "windows")]
    fn preallocate(file: &File, size: u64) -> Result<(), EngineError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{FileAllocationInfo, SetFileInformationByHandle, FILE_ALLOCATION_INFO};

        let handle = file.as_raw_handle() as isize;
        let mut info = FILE_ALLOCATION_INFO {
            AllocationSize: size as i64,
        };

        let success = unsafe {
            SetFileInformationByHandle(
                handle,
                FileAllocationInfo,
                &mut info as *mut _ as *const _,
                std::mem::size_of::<FILE_ALLOCATION_INFO>() as u32,
            )
        };

        if success == 0 {
            return Err(EngineError::FileSystemError("SetFileInformationByHandle pre-allocation failed".into()));
        }
        Ok(())
    }

    pub fn write_at_offset(&mut self, offset: usize, data: &[u8]) -> Result<(), EngineError> {
        let end = offset + data.len();
        if end > self.mmap.len() {
            return Err(EngineError::FileSystemError("Memory-mapped write exceeds buffer boundaries".into()));
        }
        
        self.mmap[offset..end].copy_from_slice(data);
        Ok(())
    }
}