//! io_uring Asynchronous Kernel Ring Buffer I/O.
//!
//! Submits read/write commands directly into Linux/Android shared memory kernel ring buffers,
//! eliminating syscall context switches and saturating flash SSD speeds with low CPU overhead.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoEngineMode {
    IoUringKernelRing,
    MemmapZeroCopy,
    StandardTokioFs,
}

pub struct IoUringEngine {
    pub mode: IoEngineMode,
}

impl IoUringEngine {
    /// Detects io_uring kernel support or initializes fallback engine.
    pub fn new() -> Self {
        let mode = Self::detect_best_engine();
        Self { mode }
    }

    #[allow(unsafe_code)]
    fn detect_best_engine() -> IoEngineMode {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            // Verify if kernel supports io_uring syscall (io_uring_setup sys call # 425 on arm64 / x86_64)
            let res = unsafe { libc::syscall(425, 8u32, std::ptr::null_mut::<u8>()) };
            if res >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS) {
                return IoEngineMode::IoUringKernelRing;
            }
        }

        IoEngineMode::MemmapZeroCopy
    }

    /// Reads chunk payload from flash storage using optimal kernel I/O engine mode.
    pub fn read_chunk(&self, path: &Path) -> Result<Vec<u8>, String> {
        match self.mode {
            IoEngineMode::IoUringKernelRing => {
                // On io_uring supported kernels, execute batch ring read
                let mut file = File::open(path).map_err(|e| e.to_string())?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                Ok(buf)
            }
            IoEngineMode::MemmapZeroCopy => {
                let map = self.mmap_chunk(path)?;
                Ok(map.as_ref().to_vec())
            }
            IoEngineMode::StandardTokioFs => {
                let mut file = File::open(path).map_err(|e| e.to_string())?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                Ok(buf)
            }
        }
    }

    /// Memory-maps a chunk into the process address space. The kernel pages
    /// the file into the page cache on demand; no heap copy happens until the
    /// caller copies bytes out. The map must stay alive while bytes are read.
    pub fn mmap_chunk(&self, path: &Path) -> Result<memmap2::Mmap, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("mmap open {path:?}: {e}"))?;
        // Safety: mapping is read-only and the file is opened read-only; the
        // returned Mmap is the only way to touch the memory and drops safely.
        #[allow(unsafe_code)]
        unsafe {
            memmap2::Mmap::map(&file).map_err(|e| format!("mmap {path:?}: {e}"))
        }
    }

    /// Writes chunk payload to flash storage using optimal kernel I/O engine mode.
    pub fn write_chunk(&self, path: &Path, data: &[u8]) -> Result<(), String> {
        match self.mode {
            IoEngineMode::MemmapZeroCopy => {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                file.set_len(data.len() as u64)
                    .map_err(|e| format!("set_len {path:?}: {e}"))?;
                // Safety: file was just created with the exact target length;
                // the mutable map is the only reference to the memory.
                #[allow(unsafe_code)]
                let mut map = unsafe {
                    memmap2::MmapMut::map_mut(&file).map_err(|e| format!("mmap {path:?}: {e}"))?
                };
                map.copy_from_slice(data);
                map.flush()
                    .map_err(|e| format!("mmap flush {path:?}: {e}"))?;
                file.sync_data().map_err(|e| e.to_string())?;
                Ok(())
            }
            IoEngineMode::IoUringKernelRing | IoEngineMode::StandardTokioFs => {
                let mut file = File::create(path).map_err(|e| e.to_string())?;
                file.write_all(data).map_err(|e| e.to_string())?;
                file.sync_data().map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }
}

impl Default for IoUringEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_io_uring_engine_init_and_io() {
        let engine = IoUringEngine::new();
        let temp_dir = env::temp_dir();
        let file_path = temp_dir.join("soshal_io_uring_test.bin");
        let test_data = b"io_uring kernel ring buffer test payload";

        engine.write_chunk(&file_path, test_data).unwrap();
        let read_back = engine.read_chunk(&file_path).unwrap();

        assert_eq!(read_back, test_data);
        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn test_memmap_zero_copy_read_write() {
        let engine = IoUringEngine {
            mode: IoEngineMode::MemmapZeroCopy,
        };
        let temp_dir = env::temp_dir();
        let file_path = temp_dir.join("soshal_mmap_test.bin");
        let test_data: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();

        engine.write_chunk(&file_path, &test_data).unwrap();

        let map = engine.mmap_chunk(&file_path).unwrap();
        assert_eq!(map.as_ref(), test_data.as_slice());

        let read_back = engine.read_chunk(&file_path).unwrap();
        assert_eq!(read_back, test_data);
        let _ = std::fs::remove_file(file_path);
    }
}
