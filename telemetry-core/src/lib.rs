//! Flight recorder: encrypted circular event log in a memory-mapped file.
//!
//! Writes are compact (len-prefixed binary records), cheap, and survive
//! process death — the mmap is msynced on dump/crash-mark. A crash yields a
//! time-travel transcript of the events leading up to it.
//!
//! Encryption: XChaCha20-Poly1305 (aead) with a per-install key held in a
//! 0600 sidecar file. Purpose: keep casual readers and log aggregators out;
//! threat model mirrors SQLite-at-rest, not full-disk.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

pub mod ring;

const HEADER_LEN: usize = 4096;
const MAGIC: &[u8; 8] = b"SOSHTELE";
const VERSION: u32 = 1;
/// Export envelope magic (encrypted dump).
pub const DUMP_MAGIC: &[u8; 8] = b"SOSHDMP1";
/// Export envelope: magic || nonce(12) || ciphertext+tag.
pub const DUMP_HEADER_LEN: usize = 8 + 12;

const ENTRY_MAX_PAYLOAD: usize = 4096;
const ENTRY_HEADER: usize = 4 + 1 + 8 + 4; // total + kind + ts + payload_len
const ENTRY_MAX_TOTAL: usize = ENTRY_HEADER + ENTRY_MAX_PAYLOAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecorderInfo {
    pub capacity_bytes: usize,
    pub used_bytes: usize,
    pub entries: usize,
    pub sealed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    State = 1,
    Ipc = 2,
    Network = 3,
    Ffi = 4,
    App = 5,
    Crash = 6,
}

impl RecordKind {
    fn from_u8(v: u8) -> Option<RecordKind> {
        match v {
            1 => Some(RecordKind::State),
            2 => Some(RecordKind::Ipc),
            3 => Some(RecordKind::Network),
            4 => Some(RecordKind::Ffi),
            5 => Some(RecordKind::App),
            6 => Some(RecordKind::Crash),
            _ => None,
        }
    }
}

// Header layout (bytes):
//   0..8   magic
//   8..12  version (u32 LE)
//   12..16 capacity (u32 LE)
//   16..24 data_start (u64 LE, always HEADER_LEN)
//   24     sealed (u8)
//   25..33 head (u64 LE) — absolute file offset of next write cursor
//   33..41 len (u64 LE)  — valid bytes in ring (capped at capacity)
//   41..(HEADER_LEN-8)   reserved, zero
//   HEADER_LEN-8..HEADER_LEN  sha256(header[..HEADER_LEN-8])[..8]

struct HeaderView {
    head: u64,
    len: u64,
    sealed: bool,
}

pub struct Recorder {
    mmap: MmapMut,
    key: Key,
    key_path: PathBuf,
}

impl Recorder {
    /// Open (or create) the recorder at `path`, with `capacity_bytes` of ring
    /// space (min 64 KiB). Corrupt/foreign files are reset.
    pub fn init(path: &Path, capacity_bytes: usize) -> Result<Recorder, String> {
        let cap = capacity_bytes.max(64 * 1024);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("open recorder: {e}"))?;
        let total = HEADER_LEN + cap;
        if file.metadata().map_err(|e| e.to_string())?.len() != total as u64 {
            file.set_len(total as u64)
                .map_err(|e| format!("resize recorder: {e}"))?;
        }
        // Safety: the map is backed by a file we opened read/write; lifetime
        // is tied to this Recorder and the map is never exposed.
        #[allow(unsafe_code)]
        let mut mmap = unsafe {
            memmap2::MmapOptions::new()
                .len(total)
                .map_mut(&file)
                .map_err(|e| format!("mmap recorder: {e}"))?
        };
        if !header_valid(&mmap) {
            write_header(&mut mmap, cap);
        }
        let key_path = key_sidecar_path(path);
        let key = load_or_create_key(&key_path)?;
        Ok(Recorder {
            mmap,
            key,
            key_path,
        })
    }

    /// Append a record. Returns Err when the recorder is sealed.
    pub fn record(&mut self, kind: RecordKind, msg: &str) -> Result<(), String> {
        self.record_bytes(kind, msg.as_bytes())
    }

    pub fn record_bytes(&mut self, kind: RecordKind, payload: &[u8]) -> Result<(), String> {
        if payload.len() > ENTRY_MAX_PAYLOAD {
            return Err("record payload too large".to_string());
        }
        let hdr = read_header(&self.mmap)?;
        if hdr.sealed {
            return Err("recorder sealed".to_string());
        }
        let ts = soshal_common_core::util::now_ms();
        let total = (ENTRY_HEADER + payload.len() + 3) & !3; // pad to 4B
        let mut entry = Vec::with_capacity(total);
        entry.extend_from_slice(&(total as u32).to_le_bytes());
        entry.push(kind as u8);
        entry.extend_from_slice(&ts.to_le_bytes());
        entry.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        entry.extend_from_slice(payload);
        entry.resize(total, 0);

        let data_start = HEADER_LEN as u64;
        let cap = self.mmap.len() as u64 - data_start;
        let write_pos = hdr.head.max(data_start);
        let fits = write_pos + total as u64 <= self.mmap.len() as u64;
        if !fits {
            // Wrap: terminate the stale tail with a zero-length entry, then
            // restart at the start of the data region.
            self.mmap[write_pos as usize..write_pos as usize + 4].copy_from_slice(&[0u8; 4]);
            self.write_entry(&entry, data_start);
            self.write_header_state(data_start + total as u64, total as u64);
        } else {
            self.write_entry(&entry, write_pos);
            self.write_header_state(write_pos + total as u64, (hdr.len + total as u64).min(cap));
        }
        Ok(())
    }

    fn write_entry(&mut self, entry: &[u8], pos: u64) {
        self.mmap[pos as usize..pos as usize + entry.len()].copy_from_slice(entry);
    }

    fn write_header_state(&mut self, head: u64, len: u64) {
        self.mmap[24] = 0;
        self.mmap[25..33].copy_from_slice(&head.to_le_bytes());
        self.mmap[33..41].copy_from_slice(&len.to_le_bytes());
        let digest = checksum(&self.mmap[..HEADER_LEN - 8]);
        self.mmap[HEADER_LEN - 8..HEADER_LEN].copy_from_slice(&digest);
    }

    /// Seal the recorder (crash marker): no further writes, flush to disk.
    pub fn mark_crash(&mut self, reason: &str) -> Result<(), String> {
        self.record_bytes(RecordKind::Crash, reason.as_bytes())?;
        let hdr = read_header(&self.mmap)?;
        self.mmap[24] = 1;
        self.mmap[25..33].copy_from_slice(&hdr.head.to_le_bytes());
        self.mmap[33..41].copy_from_slice(&hdr.len.to_le_bytes());
        let digest = checksum(&self.mmap[..HEADER_LEN - 8]);
        self.mmap[HEADER_LEN - 8..HEADER_LEN].copy_from_slice(&digest);
        self.mmap
            .flush()
            .map_err(|e| format!("flush recorder: {e}"))
    }

    pub fn is_sealed(&self) -> bool {
        read_header(&self.mmap).map(|h| h.sealed).unwrap_or(false)
    }

    /// Export the whole ring encrypted: DUMP_MAGIC || nonce(12) || ct.
    pub fn dump_encrypted(&mut self) -> Result<Vec<u8>, String> {
        self.mmap
            .flush()
            .map_err(|e| format!("flush recorder: {e}"))?;
        let hdr = read_header(&self.mmap)?;
        let data_start = HEADER_LEN as u64;
        let mut plain = Vec::with_capacity(16 + hdr.len as usize);
        plain.extend_from_slice(&hdr.head.to_le_bytes());
        plain.extend_from_slice(&hdr.len.to_le_bytes());
        let begin = data_start as usize;
        let end = begin + hdr.len as usize;
        plain.extend_from_slice(&self.mmap[begin..end]);
        let nonce_bytes: [u8; 12] = random_nonce()?;
        let cipher = ChaCha20Poly1305::new(&self.key);
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plain.as_ref())
            .map_err(|e| format!("encrypt dump: {e}"))?;
        let mut out = Vec::with_capacity(DUMP_HEADER_LEN + ct.len());
        out.extend_from_slice(DUMP_MAGIC);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Decrypt a dump produced by `dump_encrypted`.
    pub fn decrypt_dump(&self, dump: &[u8]) -> Result<Vec<u8>, String> {
        if dump.len() < DUMP_HEADER_LEN || dump[..8] != DUMP_MAGIC[..] {
            return Err("bad dump envelope".to_string());
        }
        let nonce_bytes: [u8; 12] = dump[8..20]
            .try_into()
            .map_err(|_| "bad nonce".to_string())?;
        let cipher = ChaCha20Poly1305::new(&self.key);
        cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), &dump[20..])
            .map_err(|_| "dump decryption failed".to_string())
    }

    /// Reset the ring (keep key).
    pub fn clear(&mut self) -> Result<(), String> {
        let data_start = HEADER_LEN as u64;
        self.mmap[data_start as usize..].fill(0);
        self.write_header_state(data_start, 0);
        self.mmap
            .flush()
            .map_err(|e| format!("flush recorder: {e}"))
    }

    pub fn info(&self) -> Result<RecorderInfo, String> {
        let hdr = read_header(&self.mmap)?;
        Ok(RecorderInfo {
            capacity_bytes: self.mmap.len() - HEADER_LEN,
            used_bytes: hdr.len as usize,
            entries: count_entries(&self.mmap, hdr.len),
            sealed: hdr.sealed,
        })
    }

    pub fn key_path(&self) -> &Path {
        &self.key_path
    }

    /// Enumerate records (kind, timestamp, payload) for the in-app viewer.
    pub fn read_all(&self) -> Result<Vec<(RecordKind, u64, Vec<u8>)>, String> {
        let hdr = read_header(&self.mmap)?;
        let data_start = HEADER_LEN as u64;
        let end = data_start + hdr.len;
        let mut out = Vec::new();
        let mut pos = data_start;
        while pos + 4 <= end {
            let total = u32::from_le_bytes(
                self.mmap[pos as usize..pos as usize + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            if total == 0 || total > ENTRY_MAX_TOTAL || pos + total as u64 > end {
                break;
            }
            let kind = RecordKind::from_u8(self.mmap[pos as usize + 4]);
            let ts = u64::from_le_bytes(
                self.mmap[pos as usize + 5..pos as usize + 13]
                    .try_into()
                    .unwrap(),
            );
            let plen = u32::from_le_bytes(
                self.mmap[pos as usize + 13..pos as usize + 17]
                    .try_into()
                    .unwrap(),
            ) as usize;
            if plen > ENTRY_MAX_PAYLOAD || ENTRY_HEADER + plen > total {
                break;
            }
            let payload =
                self.mmap[pos as usize + ENTRY_HEADER..pos as usize + ENTRY_HEADER + plen].to_vec();
            if let Some(k) = kind {
                out.push((k, ts, payload));
            }
            pos += total as u64;
        }
        Ok(out)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.key.zeroize();
        let _ = self.mmap.flush();
    }
}

fn key_sidecar_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".key");
    PathBuf::from(os)
}

fn load_or_create_key(path: &Path) -> Result<Key, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("open recorder key: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    let mut buf = [0u8; 32];
    let n = file
        .read(&mut buf)
        .map_err(|e| format!("read recorder key: {e}"))?;
    if n != 32 {
        getrandom::fill(&mut buf).map_err(|e| format!("key entropy: {e}"))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("seek recorder key: {e}"))?;
        file.write_all(&buf)
            .map_err(|e| format!("write recorder key: {e}"))?;
    }
    Ok(*Key::from_slice(&buf))
}

fn random_nonce() -> Result<[u8; 12], String> {
    let mut n = [0u8; 12];
    getrandom::fill(&mut n).map_err(|e| format!("nonce entropy: {e}"))?;
    Ok(n)
}

fn checksum(data: &[u8]) -> [u8; 8] {
    let d = soshal_crypto_core::hash::sha256(data);
    let mut out = [0u8; 8];
    out.copy_from_slice(&d[..8]);
    out
}

fn write_header(mmap: &mut MmapMut, capacity: usize) {
    mmap.fill(0);
    mmap[..8].copy_from_slice(MAGIC);
    mmap[8..12].copy_from_slice(&VERSION.to_le_bytes());
    mmap[12..16].copy_from_slice(&(capacity as u32).to_le_bytes());
    mmap[16..24].copy_from_slice(&(HEADER_LEN as u64).to_le_bytes());
    mmap[24] = 0;
    mmap[25..33].copy_from_slice(&(HEADER_LEN as u64).to_le_bytes());
    mmap[33..41].copy_from_slice(&0u64.to_le_bytes());
    let digest = checksum(&mmap[..HEADER_LEN - 8]);
    mmap[HEADER_LEN - 8..HEADER_LEN].copy_from_slice(&digest);
}

fn read_header(mmap: &MmapMut) -> Result<HeaderView, String> {
    if mmap.len() < HEADER_LEN || &mmap[..8] != MAGIC {
        return Err("recorder header invalid".to_string());
    }
    let digest = checksum(&mmap[..HEADER_LEN - 8]);
    if mmap[HEADER_LEN - 8..HEADER_LEN] != digest {
        return Err("recorder header checksum failed".to_string());
    }
    let sealed = mmap[24] != 0;
    let head = u64::from_le_bytes(mmap[25..33].try_into().unwrap());
    let len = u64::from_le_bytes(mmap[33..41].try_into().unwrap());
    Ok(HeaderView { head, len, sealed })
}

fn header_valid(mmap: &MmapMut) -> bool {
    read_header(mmap).is_ok()
}

fn count_entries(mmap: &MmapMut, len: u64) -> usize {
    let data_start = HEADER_LEN as u64;
    let end = data_start + len;
    let mut pos = data_start;
    let mut n = 0usize;
    while pos + 4 <= end {
        let total =
            u32::from_le_bytes(mmap[pos as usize..pos as usize + 4].try_into().unwrap()) as usize;
        if total == 0 || total > ENTRY_MAX_TOTAL || pos + total as u64 > end {
            break;
        }
        n += 1;
        pos += total as u64;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_and_info() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let mut r = Recorder::init(&p, 5 * 1024 * 1024).unwrap();
        assert_eq!(r.info().unwrap().capacity_bytes, 5 * 1024 * 1024);
        assert!(!r.is_sealed());
        r.clear().unwrap();
    }

    #[test]
    fn record_roundtrip() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        r.record(RecordKind::State, "signed-in").unwrap();
        r.record(RecordKind::Network, "relay connected").unwrap();
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, RecordKind::State);
        assert_eq!(entries[0].2, b"signed-in");
        assert_eq!(entries[1].0, RecordKind::Network);
    }

    #[test]
    fn wrap_around() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        let big = "x".repeat(2000);
        for _ in 0..500 {
            r.record(RecordKind::App, &big).unwrap();
        }
        let info = r.info().unwrap();
        assert!(info.used_bytes <= info.capacity_bytes);
        assert!(info.entries >= 20, "entries: {}", info.entries);
        assert!(info.entries <= 30);
        // Records must still parse cleanly after wrapping.
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), info.entries);
    }

    #[test]
    fn crash_seals_and_blocks_writes() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        r.mark_crash("fatal: null deref").unwrap();
        assert!(r.is_sealed());
        assert!(r.record(RecordKind::App, "late").is_err());
        let entries = r.read_all().unwrap();
        assert!(entries
            .iter()
            .any(|(k, _, msg)| *k == RecordKind::Crash && msg == b"fatal: null deref"));
    }

    #[test]
    fn dump_encrypt_decrypt() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        r.record(RecordKind::Ipc, "ffi-call-42").unwrap();
        r.record(RecordKind::State, "locked").unwrap();
        let dump = r.dump_encrypted().unwrap();
        assert_eq!(&dump[..8], &DUMP_MAGIC[..]);
        let plain = r.decrypt_dump(&dump).unwrap();
        assert!(plain.windows(11).any(|w| w == b"ffi-call-42"));
    }

    #[test]
    fn corruption_resets() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        {
            let mut r = Recorder::init(&p, 64 * 1024).unwrap();
            r.record(RecordKind::App, "pre-corruption").unwrap();
        }
        let mut f = OpenOptions::new().write(true).open(&p).unwrap();
        f.seek(SeekFrom::Start(0)).unwrap();
        f.write_all(b"garbage").unwrap();
        drop(f);
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        r.record(RecordKind::App, "fresh start").unwrap();
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn clear_wipes_entries() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        r.record(RecordKind::App, "one").unwrap();
        r.clear().unwrap();
        assert_eq!(r.read_all().unwrap().len(), 0);
    }

    #[test]
    fn key_file_persists_across_reopen() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        {
            let mut r = Recorder::init(&p, 64 * 1024).unwrap();
            r.record(RecordKind::State, "session-a").unwrap();
            let dump = r.dump_encrypted().unwrap();
            let r2 = Recorder::init(&p, 64 * 1024).unwrap();
            let plain = r2.decrypt_dump(&dump).unwrap();
            assert!(plain.windows(9).any(|w| w == b"session-a"));
        }
    }

    #[test]
    fn wrong_key_fails_decrypt() {
        let p = soshal_test_util::tmp_path("recorder", "rec.bin");
        let p2 = soshal_test_util::tmp_path("recorder", "rec2.bin");
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        r.record(RecordKind::State, "secret-a").unwrap();
        let dump = r.dump_encrypted().unwrap();
        let r2 = Recorder::init(&p2, 64 * 1024).unwrap();
        assert!(r2.decrypt_dump(&dump).is_err());
    }

    #[test]
    fn read_all_stops_at_corrupt_entry() {
        // Corrupt total field: scan breaks, valid prefix survives.
        let p = soshal_test_util::tmp_path("recorder", "corrupt-total.bin");
        {
            let mut r = Recorder::init(&p, 64 * 1024).unwrap();
            r.record(RecordKind::State, "before").unwrap();
            r.record(RecordKind::App, "corrupted").unwrap();
            r.record(RecordKind::App, "after").unwrap();
        }
        let mut f = OpenOptions::new().write(true).open(&p).unwrap();
        // entry1 total = (17+6+3)&!3 = 24 → entry2 total field at +24.
        f.seek(SeekFrom::Start(HEADER_LEN as u64 + 24)).unwrap();
        f.write_all(&[0xFF; 4]).unwrap();
        drop(f);
        let r = Recorder::init(&p, 64 * 1024).unwrap();
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].2, b"before");

        // Corrupt plen field: same break behavior.
        let p2 = soshal_test_util::tmp_path("recorder", "corrupt-plen.bin");
        {
            let mut r = Recorder::init(&p2, 64 * 1024).unwrap();
            r.record(RecordKind::State, "before").unwrap();
            r.record(RecordKind::App, "corrupted").unwrap();
        }
        let mut f = OpenOptions::new().write(true).open(&p2).unwrap();
        // entry2 plen field at entry2 (+24) + 13.
        f.seek(SeekFrom::Start(HEADER_LEN as u64 + 24 + 13))
            .unwrap();
        f.write_all(&[0xFF; 4]).unwrap();
        drop(f);
        let r = Recorder::init(&p2, 64 * 1024).unwrap();
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].2, b"before");
    }

    #[test]
    fn read_all_skips_unknown_kind() {
        let p = soshal_test_util::tmp_path("recorder", "unknown-kind.bin");
        {
            let mut r = Recorder::init(&p, 64 * 1024).unwrap();
            r.record(RecordKind::State, "one").unwrap();
            r.record(RecordKind::Ipc, "two").unwrap();
            r.record(RecordKind::App, "three").unwrap();
        }
        let mut f = OpenOptions::new().write(true).open(&p).unwrap();
        // entry1 total = (17+3+3)&!3 = 20 → entry2 kind byte at +20+4.
        f.seek(SeekFrom::Start(HEADER_LEN as u64 + 20 + 4)).unwrap();
        f.write_all(&[7u8]).unwrap();
        drop(f);
        let r = Recorder::init(&p, 64 * 1024).unwrap();
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, RecordKind::State);
        assert_eq!(entries[0].2, b"one");
        assert_eq!(entries[1].0, RecordKind::App);
        assert_eq!(entries[1].2, b"three");
    }

    #[test]
    fn decrypt_dump_rejects_bad_envelope() {
        let p = soshal_test_util::tmp_path("recorder", "decrypt-env.bin");
        let r = Recorder::init(&p, 64 * 1024).unwrap();
        assert_eq!(r.decrypt_dump(b"garbage").unwrap_err(), "bad dump envelope");
        assert_eq!(r.decrypt_dump(&[0u8; 19]).unwrap_err(), "bad dump envelope");
        assert_eq!(r.decrypt_dump(&[0u8; 20]).unwrap_err(), "bad dump envelope");
    }

    #[test]
    fn init_edge_cases() {
        // Truncated key file (16 bytes) → regenerated 32-byte key.
        let p = soshal_test_util::tmp_path("recorder", "trunc-key.bin");
        let dump = {
            let mut r = Recorder::init(&p, 64 * 1024).unwrap();
            r.record(RecordKind::State, "pre-trunc").unwrap();
            r.dump_encrypted().unwrap()
        };
        let kp = key_sidecar_path(&p);
        {
            let mut kf = OpenOptions::new().write(true).open(&kp).unwrap();
            kf.set_len(16).unwrap();
        }
        let mut r = Recorder::init(&p, 64 * 1024).unwrap();
        assert_eq!(std::fs::metadata(&kp).unwrap().len(), 32);
        assert!(r.decrypt_dump(&dump).is_err()); // key changed
        r.record(RecordKind::App, "post-trunc").unwrap();
        let entries = r.read_all().unwrap();
        assert_eq!(entries.len(), 2); // old ring data survives
        assert_eq!(entries[1].2, b"post-trunc");

        // Double seal: second mark_crash errors, no extra crash record.
        let p2 = soshal_test_util::tmp_path("recorder", "double-seal.bin");
        let mut r2 = Recorder::init(&p2, 64 * 1024).unwrap();
        r2.mark_crash("boom").unwrap();
        assert_eq!(r2.mark_crash("boom again").unwrap_err(), "recorder sealed");
        assert!(r2.is_sealed());
        let entries = r2.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, RecordKind::Crash);
        assert_eq!(entries[0].2, b"boom");

        // Pre-existing odd-length file → resized to header + capacity.
        let p3 = soshal_test_util::tmp_path("recorder", "odd-size.bin");
        std::fs::write(&p3, vec![0xABu8; 5001]).unwrap();
        let mut r3 = Recorder::init(&p3, 64 * 1024).unwrap();
        assert_eq!(
            std::fs::metadata(&p3).unwrap().len() as usize,
            HEADER_LEN + 64 * 1024
        );
        r3.record(RecordKind::App, "post-resize").unwrap();
        assert_eq!(r3.read_all().unwrap().len(), 1);
    }
}
