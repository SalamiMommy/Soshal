//! io_uring Asynchronous Kernel Ring Buffer I/O.
//!
//! Submits read/write commands directly into Linux/Android kernel io_uring
//! rings, eliminating syscall context switches for chunk I/O. When the
//! kernel does not support io_uring (disabled via sysctl/seccomp, older
//! kernels, non-Linux), the engine falls back to mmap zero-copy.

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

/// Queue depth for the io_uring ring.
const RING_DEPTH: u32 = 8;

impl IoUringEngine {
    /// Detects io_uring kernel support or initializes fallback engine.
    pub fn new() -> Self {
        let mode = Self::detect_best_engine();
        Self { mode }
    }

    fn detect_best_engine() -> IoEngineMode {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            // io_uring_setup succeeds only when the kernel supports the
            // feature; any error (ENOSYS, EPERM when disabled via sysctl,
            // seccomp denial) means the fallback engine must be used.
            if io_uring::IoUring::new(RING_DEPTH).is_ok() {
                return IoEngineMode::IoUringKernelRing;
            }
        }
        IoEngineMode::MemmapZeroCopy
    }

    /// Reads chunk payload from flash storage using optimal kernel I/O engine mode.
    pub fn read_chunk(&self, path: &Path) -> Result<Vec<u8>, String> {
        match self.mode {
            IoEngineMode::IoUringKernelRing => self.read_uring(path),
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
            IoEngineMode::IoUringKernelRing => self.write_uring(path, data),
            IoEngineMode::StandardTokioFs => {
                let mut file = File::create(path).map_err(|e| e.to_string())?;
                file.write_all(data).map_err(|e| e.to_string())?;
                file.sync_data().map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    /// io_uring-backed read: submits one read per in-flight request and
    /// waits for each completion, so no extra heap copies happen beyond the
    /// caller-owned buffer.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn read_uring(&self, path: &Path) -> Result<Vec<u8>, String> {
        use io_uring::{opcode, types};
        use std::os::unix::io::AsRawFd;

        let file = File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
        let size = file.metadata().map_err(|e| e.to_string())?.len() as usize;
        if size == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; size];
        let mut ring = io_uring::IoUring::new(RING_DEPTH).map_err(|e| e.to_string())?;
        let fd = types::Fd(file.as_raw_fd());
        let mut done = 0usize;
        while done < size {
            let entry = opcode::Read::new(fd, buf[done..].as_mut_ptr(), (size - done) as u32)
                .offset(done as u64)
                .build()
                .user_data(1);
            // Safety: `buf` is a caller-owned heap buffer that outlives the
            // synchronous submission/completion cycle below; the read is
            // fully synchronized (submit_and_wait) before the buffer is
            // touched again.
            #[allow(unsafe_code)]
            unsafe {
                ring.submission().push(&entry).map_err(|e| e.to_string())?;
            }
            ring.submit_and_wait(1).map_err(|e| e.to_string())?;
            let cqe = ring.completion().next().ok_or("io_uring: no completion")?;
            let res = cqe.result();
            if res < 0 {
                return Err(std::io::Error::from_raw_os_error(-res).to_string());
            }
            done += res as usize;
        }
        Ok(buf)
    }

    /// io_uring-backed write: truncates the file, then writes all bytes
    /// through the ring and fsyncs.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn write_uring(&self, path: &Path, data: &[u8]) -> Result<(), String> {
        use io_uring::{opcode, types};
        use std::os::unix::io::AsRawFd;

        let file = File::create(path).map_err(|e| format!("create {path:?}: {e}"))?;
        if data.is_empty() {
            return file.sync_data().map_err(|e| e.to_string());
        }
        let mut ring = io_uring::IoUring::new(RING_DEPTH).map_err(|e| e.to_string())?;
        let fd = types::Fd(file.as_raw_fd());
        let mut done = 0usize;
        while done < data.len() {
            let entry = opcode::Write::new(fd, data[done..].as_ptr(), (data.len() - done) as u32)
                .offset(done as u64)
                .build()
                .user_data(1);
            // Safety: `data` is the caller's slice and outlives the
            // synchronous submission/completion cycle.
            #[allow(unsafe_code)]
            unsafe {
                ring.submission().push(&entry).map_err(|e| e.to_string())?;
            }
            ring.submit_and_wait(1).map_err(|e| e.to_string())?;
            let cqe = ring.completion().next().ok_or("io_uring: no completion")?;
            let res = cqe.result();
            if res < 0 {
                return Err(std::io::Error::from_raw_os_error(-res).to_string());
            }
            done += res as usize;
        }
        file.sync_data().map_err(|e| e.to_string())
    }

    /// Non-Linux fallback: io_uring is unavailable, so the kernel-ring mode
    /// degrades to mmap zero-copy semantics (never a silent plain read).
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn read_uring(&self, path: &Path) -> Result<Vec<u8>, String> {
        let map = self.mmap_chunk(path)?;
        Ok(map.as_ref().to_vec())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn write_uring(&self, path: &Path, data: &[u8]) -> Result<(), String> {
        let mut file = File::create(path).map_err(|e| e.to_string())?;
        file.write_all(data).map_err(|e| e.to_string())?;
        file.sync_data().map_err(|e| e.to_string())?;
        Ok(())
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
    fn test_kernel_ring_mode_reads_and_writes() {
        let engine = IoUringEngine {
            mode: IoEngineMode::IoUringKernelRing,
        };
        let temp_dir = env::temp_dir();
        let file_path = temp_dir.join("soshal_uring_mode_test.bin");
        let test_data: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();

        engine.write_chunk(&file_path, &test_data).unwrap();
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
