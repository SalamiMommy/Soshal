//! Zero-IPC shared-memory ring buffer for high-frequency telemetry.
//!
//! Dart writes analytics events directly into a memory-mapped region (raw
//! `Pointer<Uint8>` writes — zero FFI call overhead per event) and advances a
//! monotonic head cursor with one cheap FFI call per event. A Tokio task
//! consumes from the tail, compresses, and flushes — UI frames never wait.
//!
//! Single-writer single-consumer discipline:
//! - Writer (Dart): writes the entry bytes, THEN advances head. Consumers
//!   never read past head, so torn writes are unobservable.
//! - Consumer (Rust): drains up to head, then advances tail.
//! - Entries are len-prefixed (u32 total, padded to 4B). A zero word at the
//!   current index marks the wrap point; consumers skip to the next boundary.
//! - Full ring: writer overwrites oldest entries (lossy by design — analytics
//!   tolerate sampling loss; telemetry never blocks the UI).
//!
//! The region is a plain mmap (optionally file-backed) whose address is
//! shared with Dart as a usize; no kernel IPC, no copy.

use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::path::Path;

/// Shared header layout (bytes):
///   0..8   head (u64 LE) — next writer cursor (absolute)
///   8..16  tail (u64 LE) — next consumer cursor (absolute)
///   16..20 capacity (u32 LE)
///   20..4096 reserved
///   4096+  data ring
pub const RING_HEADER_LEN: usize = 4096;
pub const RING_ENTRY_MAX_PAYLOAD: usize = 4096;
const RING_ENTRY_HEADER: usize = 4 + 1 + 8 + 4; // total + kind + ts + payload_len

/// Result of draining: one telemetry entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainedEntry {
    pub kind: u8,
    pub payload: Vec<u8>,
}

pub struct SharedRing {
    map: MmapMut,
    capacity: usize,
}

impl SharedRing {
    /// Opens (or creates) the ring at `path` with `capacity_bytes` of data
    /// space (min 64 KiB, rounded up to a 4096 multiple).
    pub fn init(path: &Path, capacity_bytes: usize) -> Result<Self, String> {
        let cap = capacity_bytes.max(64 * 1024).div_ceil(4096) * 4096;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("ring open: {e}"))?;
        let total = RING_HEADER_LEN + cap;
        if file.metadata().map_err(|e| e.to_string())?.len() != total as u64 {
            file.set_len(total as u64)
                .map_err(|e| format!("ring resize: {e}"))?;
        }
        // Safety: the map is backed by the file we opened read/write; the
        // SharedRing is the sole owner and drops unmap cleanly.
        #[allow(unsafe_code)]
        let mut map = unsafe {
            memmap2::MmapOptions::new()
                .len(total)
                .map_mut(&file)
                .map_err(|e| format!("ring mmap: {e}"))?
        };
        let existing = read_u64(&map, 0);
        if existing == 0 {
            map[0..8].copy_from_slice(&0u64.to_le_bytes());
            map[8..16].copy_from_slice(&0u64.to_le_bytes());
            map[16..20].copy_from_slice(&(cap as u32).to_le_bytes());
        }
        Ok(Self { map, capacity: cap })
    }

    /// Raw address handed to Dart (dart:ffi `Pointer.fromAddress`).
    pub fn addr(&self) -> usize {
        self.map.as_ptr() as usize
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn head(&self) -> u64 {
        read_u64(&self.map, 0)
    }

    /// Monotonic head advance from the writer. Rejects rewinds.
    pub fn advance_head(&mut self, new_head: u64) -> Result<(), String> {
        let old = self.head();
        if new_head < old {
            return Err(format!("head rewind: {old} -> {new_head}"));
        }
        if new_head - old > self.capacity as u64 {
            return Err("head advanced beyond capacity".to_string());
        }
        self.map[0..8].copy_from_slice(&new_head.to_le_bytes());
        Ok(())
    }

    /// Consumes all complete entries up to head, invoking `f(kind, payload)`
    /// per entry. Returns the number of entries drained.
    pub fn drain<F: FnMut(u8, &[u8])>(&mut self, mut f: F) -> usize {
        let mut tail = read_u64(&self.map, 8);
        let head = self.head();
        let mut drained = 0;
        while tail < head {
            let idx = (tail % self.capacity as u64) as usize;
            let len_word = read_u32(&self.map, RING_HEADER_LEN + idx);
            if len_word == 0 {
                // Wrap marker: skip to the next ring boundary.
                tail += (self.capacity - idx) as u64;
                continue;
            }
            let total = len_word as usize;
            if !(RING_ENTRY_HEADER..=RING_ENTRY_HEADER + RING_ENTRY_MAX_PAYLOAD).contains(&total)
                || idx + total > self.capacity
            {
                // Corrupt head region (partial write before advance is
                // impossible; this guards against foreign writers). Skip.
                tail += 4;
                continue;
            }
            let base = RING_HEADER_LEN + idx;
            let kind = self.map[base + 4];
            let payload_len =
                u32::from_le_bytes(self.map[base + 13..base + 17].try_into().unwrap()) as usize;
            let payload = if payload_len <= RING_ENTRY_MAX_PAYLOAD {
                let start = base + RING_ENTRY_HEADER;
                self.map[start..start + payload_len].to_vec()
            } else {
                Vec::new()
            };
            f(kind, &payload);
            tail += total as u64;
            drained += 1;
        }
        self.map[8..16].copy_from_slice(&tail.to_le_bytes());
        drained
    }

    /// Number of unconsumed bytes (approx; ignores per-entry padding).
    pub fn pending(&self) -> u64 {
        self.head().saturating_sub(read_u64(&self.map, 8))
    }
}

fn read_u64(map: &MmapMut, off: usize) -> u64 {
    u64::from_le_bytes(map[off..off + 8].try_into().unwrap())
}

fn read_u32(map: &MmapMut, off: usize) -> u32 {
    u32::from_le_bytes(map[off..off + 4].try_into().unwrap())
}

/// In-process registry so FFI can advance/drain by address without holding
/// the ring handle in the bridge layer.
static RINGS: std::sync::OnceLock<
    std::sync::Mutex<
        std::collections::HashMap<usize, std::sync::Arc<std::sync::Mutex<SharedRing>>>,
    >,
> = std::sync::OnceLock::new();

fn registry() -> &'static std::sync::Mutex<
    std::collections::HashMap<usize, std::sync::Arc<std::sync::Mutex<SharedRing>>>,
> {
    RINGS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Registers a ring for FFI access by address. Returns the address.
pub fn register_ring(path: &Path, capacity_bytes: usize) -> Result<usize, String> {
    let ring = SharedRing::init(path, capacity_bytes)?;
    let addr = ring.addr();
    registry()
        .lock()
        .map_err(|_| "ring registry poisoned".to_string())?
        .insert(addr, std::sync::Arc::new(std::sync::Mutex::new(ring)));
    Ok(addr)
}

/// FFI: advance the head cursor of the ring at `addr`.
pub fn ffi_advance_head(addr: usize, new_head: u64) -> Result<(), String> {
    let ring = registry()
        .lock()
        .map_err(|_| "ring registry poisoned".to_string())?
        .get(&addr)
        .ok_or_else(|| "unknown ring addr".to_string())?
        .clone();
    let mut guard = ring.lock().map_err(|_| "ring locked".to_string())?;
    guard.advance_head(new_head)
}

/// FFI: drain all complete entries, returning them (consumer owns this call).
pub fn ffi_drain(addr: usize) -> Result<Vec<DrainedEntry>, String> {
    let ring = registry()
        .lock()
        .map_err(|_| "ring registry poisoned".to_string())?
        .get(&addr)
        .ok_or_else(|| "unknown ring addr".to_string())?
        .clone();
    let mut guard = ring.lock().map_err(|_| "ring locked".to_string())?;
    let mut out = Vec::new();
    guard.drain(|kind, payload| {
        out.push(DrainedEntry {
            kind,
            payload: payload.to_vec(),
        })
    });
    Ok(out)
}

/// FFI: pending bytes for the consumer's scheduling decision.
pub fn ffi_pending(addr: usize) -> Result<u64, String> {
    let ring = registry()
        .lock()
        .map_err(|_| "ring registry poisoned".to_string())?
        .get(&addr)
        .ok_or_else(|| "unknown ring addr".to_string())?
        .clone();
    let guard = ring.lock().map_err(|_| "ring locked".to_string())?;
    Ok(guard.pending())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_entry_bytes(ring: &mut SharedRing, kind: u8, payload: &[u8]) {
        let total = (RING_ENTRY_HEADER + payload.len() + 3) & !3;
        let base = RING_HEADER_LEN + (ring.head() % ring.capacity() as u64) as usize;
        if base + total > RING_HEADER_LEN + ring.capacity() {
            // Simulate the writer's wrap marker (zero word at boundary).
            let idx = (ring.head() % ring.capacity() as u64) as usize;
            ring.map[RING_HEADER_LEN + idx..RING_HEADER_LEN + idx + 4]
                .copy_from_slice(&0u32.to_le_bytes());
        }
        let idx = (ring.head() % ring.capacity() as u64) as usize;
        let base = RING_HEADER_LEN + idx;
        ring.map[base..base + 4].copy_from_slice(&(total as u32).to_le_bytes());
        ring.map[base + 4] = kind;
        ring.map[base + 5..base + 13].copy_from_slice(&0u64.to_le_bytes());
        ring.map[base + 13..base + 17].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        ring.map[base + RING_ENTRY_HEADER..base + RING_ENTRY_HEADER + payload.len()]
            .copy_from_slice(payload);
        let new_head = ring.head() + total as u64;
        ring.advance_head(new_head).unwrap();
    }

    #[test]
    fn drain_receives_entries_in_order() {
        let mut ring =
            SharedRing::init(&soshal_test_util::tmp_path("ring", "ring.bin"), 64 * 1024).unwrap();
        write_entry_bytes(&mut ring, 5, b"hello");
        write_entry_bytes(&mut ring, 3, b"world-longer");
        write_entry_bytes(&mut ring, 2, &[0u8; 100]);

        let mut kinds = Vec::new();
        let mut payloads = Vec::new();
        ring.drain(|k, p| {
            kinds.push(k);
            payloads.push(p.to_vec());
        });
        assert_eq!(kinds, vec![5, 3, 2]);
        assert_eq!(payloads[0], b"hello");
        assert_eq!(payloads[1], b"world-longer");
        assert_eq!(payloads[2], vec![0u8; 100]);
        assert_eq!(ring.pending(), 0);
    }

    #[test]
    fn wrap_around_skips_boundary() {
        let mut ring =
            SharedRing::init(&soshal_test_util::tmp_path("ring", "ring.bin"), 4096 * 3).unwrap();
        // Fill until the writer would straddle the boundary twice.
        for i in 0..50 {
            write_entry_bytes(&mut ring, 1, &[i as u8; 500]);
        }
        let drained = ring.drain(|_, _| {});
        assert_eq!(drained, 50);
        assert_eq!(ring.pending(), 0);
    }

    #[test]
    fn registry_advance_and_drain() {
        let addr =
            register_ring(&soshal_test_util::tmp_path("ring", "ring.bin"), 64 * 1024).unwrap();
        assert!(addr > 0);
        assert!(ffi_advance_head(addr, 0).is_ok());
        let entries = ffi_drain(addr).unwrap();
        assert!(entries.is_empty());
        assert!(ffi_pending(addr).unwrap() == 0);
    }
}
