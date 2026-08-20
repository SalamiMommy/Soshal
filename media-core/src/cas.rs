//! Local content-addressed chunk store (CAS).
//!
//! Chunks live at `<root>/<first2>/<rest>.chunk` keyed by BLAKE3 hex. Writes
//! are idempotent: `put` verifies the hash matches the key before writing, so
//! a corrupted or malicious chunk can never poison a valid hash slot. Files
//! are never overwritten — a hash that exists is trusted as-is.
//!
//! Dedup is implicit: five posts sharing one audio clip produce five identical
//! chunk hashes, all resolving to a single on-disk file.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use crate::chunking::{store_reader_with_data, ChunkManifest, ChunkRef};

fn is_valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Persistent override for [`ChunkStore::default_root`], installed once by
/// the FFI bridge at `db_init` so blobs survive OS temp-dir wipes.
static DEFAULT_ROOT_OVERRIDE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Chunk files are plaintext cache content. At-rest encryption of the app DB
/// is untouched; the CAS lives in cache space and is evictable.
#[derive(Clone)]
pub struct ChunkStore {
    root: PathBuf,
    /// Lazy chunk-hash → owning manifest cache. Resolves peer "do you have
    /// chunk X" requests without re-reading every manifests/*.json.
    index: Arc<RwLock<Option<Arc<ChunkIndex>>>>,
    /// Verified (hash, size, mtime) entries; immutable chunk files skip the
    /// full re-hash on repeated reads once verified.
    verified: Arc<Mutex<VerifiedChunks>>,
    /// Manifest LRU: peer-serving re-reads manifests/*.json per chunk request
    /// (both TCP and QUIC bulk paths). Capped FIFO, invalidated on save/clear.
    manifest_cache: Arc<Mutex<FifoCache<ChunkManifest>>>,
    /// Open-chunk LRU: `blob_slice` maps each chunk on demand; caching the
    /// maps (page-cache backed) avoids a File::open + mmap syscall pair per
    /// chunk on every range read. Capped FIFO like the manifest cache.
    mmap_cache: Arc<Mutex<FifoCache<memmap2::Mmap>>>,
}

/// FIFO-capped cache of Arc'd values keyed by hash string.
struct FifoCache<T> {
    order: std::collections::VecDeque<String>,
    map: HashMap<String, Arc<T>>,
    cap: usize,
}

impl<T> Default for FifoCache<T> {
    fn default() -> Self {
        Self {
            order: std::collections::VecDeque::new(),
            map: HashMap::new(),
            cap: 0,
        }
    }
}

impl<T> FifoCache<T> {
    fn get(&self, key: &str) -> Option<Arc<T>> {
        self.map.get(key).cloned()
    }

    fn put(&mut self, key: String, value: Arc<T>) {
        if self.map.contains_key(&key) {
            return;
        }
        if self.map.len() >= self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    fn invalidate(&mut self, blob_hash: &str) {
        self.map.remove(blob_hash);
        self.order.retain(|h| h != blob_hash);
    }
}

/// `chunk_hash -> blob_hash` map plus the chunk's offset inside that blob.
/// Built once from the manifests directory, kept incrementally in sync on
/// `save_manifest`, and dropped wholesale on `clear`.
#[derive(Default, Clone)]
struct ChunkIndex {
    by_chunk: HashMap<String, Vec<(String, u64)>>,
}

impl ChunkIndex {
    fn build(root: &Path) -> ChunkIndex {
        let mut idx = ChunkIndex::default();
        let dir = root.join("manifests");
        let Ok(entries) = fs::read_dir(dir) else {
            return idx;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(m) = serde_json::from_str::<ChunkManifest>(&raw) else {
                continue;
            };
            if m.is_valid() {
                idx.ingest(&m);
            }
        }
        idx
    }

    fn ingest(&mut self, m: &ChunkManifest) {
        for c in &m.chunks {
            self.by_chunk
                .entry(c.blake3.clone())
                .or_default()
                .push((m.blob_hash.clone(), c.offset));
        }
    }
}

type VerifiedChunks = HashMap<String, (u64, Option<SystemTime>)>;

impl ChunkStore {
    /// `root` should point at the cache dir (e.g. `<cache>/chunks`).
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            index: Arc::new(RwLock::new(None)),
            verified: Arc::new(Mutex::new(HashMap::new())),
            manifest_cache: Arc::new(Mutex::new(FifoCache {
                cap: 64,
                ..Default::default()
            })),
            mmap_cache: Arc::new(Mutex::new(FifoCache {
                cap: 64,
                ..Default::default()
            })),
        }
    }

    fn index(&self) -> Arc<ChunkIndex> {
        if let Some(idx) = self
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return idx.clone();
        }
        let built = Arc::new(ChunkIndex::build(&self.root));
        if let Ok(mut slot) = self.index.write() {
            if slot.is_none() {
                *slot = Some(built);
            }
        }
        self.index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .unwrap()
            .clone()
    }

    /// Rebuild the index from scratch (used when on-disk state changed
    /// outside this store, e.g. cache eviction ran).
    pub fn refresh_index(&self) {
        if let Ok(mut slot) = self.index.write() {
            *slot = Some(Arc::new(ChunkIndex::build(&self.root)));
        }
    }

    /// Canonical on-disk path for a chunk hash. Two-char prefix subdir avoids
    /// a single directory holding thousands of files.
    pub fn chunk_path(&self, hash: &str) -> PathBuf {
        if !is_valid_hash(hash) {
            let safe_hash = if hash.len() >= 2 {
                hash.chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
            } else {
                "invalid".to_string()
            };
            let prefix = if safe_hash.len() >= 2 {
                &safe_hash[..2]
            } else {
                "00"
            };
            return self
                .root
                .join("invalid")
                .join(prefix)
                .join(&safe_hash)
                .with_extension("chunk");
        }
        let (a, b) = hash.split_at(2);
        self.root.join(a).join(b).with_extension("chunk")
    }

    pub fn contains(&self, hash: &str) -> bool {
        if !is_valid_hash(hash) {
            return false;
        }
        self.chunk_path(hash).is_file()
    }

    /// Reads a chunk and verifies its BLAKE3 hash. `None` on missing/corrupt.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        self.get_mmap(hash).map(|m| m.as_ref().to_vec())
    }

    /// Memory-maps a chunk without copying it into the heap. The returned map
    /// pages straight from disk; hash verification reads the page cache, not
    /// a user-space copy. `None` on missing/corrupt. Callers keep the map
    /// alive while they (or a consumer handed the pointer) touch the bytes.
    pub fn get_mmap(&self, hash: &str) -> Option<memmap2::Mmap> {
        if !is_valid_hash(hash) {
            return None;
        }
        let path = self.chunk_path(hash);
        let file = fs::File::open(&path).ok()?;
        let md = file.metadata().ok()?;
        let size = md.len();
        let mtime = md.modified().ok();
        // Safety: read-only mapping of a file we opened read-only; the Mmap is
        // the sole handle to the region and unmap happens on drop.
        #[allow(unsafe_code)]
        let map = unsafe { memmap2::Mmap::map(&file).ok()? };
        if self.verified_contains(hash, size, mtime) {
            return Some(map);
        }
        if blake3::hash(map.as_ref()).to_hex().as_str() != hash {
            return None;
        }
        self.mark_verified(hash.to_string(), size, mtime);
        Some(map)
    }

    /// `get_mmap` through a capped FIFO cache: `blob_slice` maps every chunk
    /// of a range, so open+mmap per chunk would syscall-spam; sequential
    /// chunks hit the cache.
    pub fn get_mmap_cached(&self, hash: &str) -> Option<Arc<memmap2::Mmap>> {
        if hash.len() != 64 {
            return None;
        }
        if let Ok(cache) = self.mmap_cache.lock() {
            if let Some(m) = cache.get(hash) {
                return Some(m);
            }
        }
        let m = Arc::new(self.get_mmap(hash)?);
        if let Ok(mut cache) = self.mmap_cache.lock() {
            cache.put(hash.to_string(), m.clone());
        }
        Some(m)
    }

    fn verified_contains(&self, hash: &str, size: u64, mtime: Option<SystemTime>) -> bool {
        self.verified
            .lock()
            .map(|c| c.get(hash).is_some_and(|(s, m)| *s == size && *m == mtime))
            .unwrap_or(false)
    }

    fn mark_verified(&self, hash: String, size: u64, mtime: Option<SystemTime>) {
        if let Ok(mut cache) = self.verified.lock() {
            if cache.len() >= 256 {
                if let Some(first_key) = cache.keys().next().cloned() {
                    cache.remove(&first_key);
                }
            }
            cache.insert(hash, (size, mtime));
        }
    }

    /// Stores a chunk. Returns `true` if newly written, `false` if it already
    /// existed (or the hash didn't match — slot is never poisoned).
    pub fn put(&self, data: &[u8]) -> bool {
        let hash = blake3::hash(data).to_hex().to_string();
        self.put_trusted(&hash, data)
    }

    /// Stores a chunk under a trusted pre-calculated hash without re-hashing data.
    pub fn put_trusted(&self, hash: &str, data: &[u8]) -> bool {
        if !is_valid_hash(hash) {
            return false;
        }
        let path = self.chunk_path(hash);
        if path.is_file() {
            return false;
        }
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return false;
            }
        }
        let mut tmp = path.with_extension("tmp");
        let mut n = 0u32;
        while tmp.exists() {
            n += 1;
            tmp = format!("{}.{n}", path.to_string_lossy()).into();
        }
        if fs::File::create(&tmp)
            .and_then(|mut f| f.write_all(data))
            .and_then(|_| fs::rename(&tmp, &path))
            .is_err()
        {
            let _ = fs::remove_file(&tmp);
            return false;
        }
        true
    }

    /// Stores a chunk under an expected hash, verifying the content first.
    pub fn put_verified(&self, expected: &str, data: &[u8]) -> bool {
        if expected.len() != 64 || blake3::hash(data).to_hex().as_str() != expected {
            return false;
        }
        self.put_trusted(expected, data)
    }

    /// Chunks a file on disk and stores every chunk, deduplicating as it goes.
    /// Returns the manifest (empty-file safe).
    pub fn store_file(&self, path: &Path) -> Result<ChunkManifest, String> {
        let file = fs::File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
        self.store_reader(file)
    }

    /// Chunks any reader and stores the chunks, deduplicating as it goes.
    /// Chunks are written and dropped one at a time — the whole blob is never
    /// held in RAM.
    pub fn store_reader<R: Read>(&self, reader: R) -> Result<ChunkManifest, String> {
        store_reader_with_data(reader, |chr, data| {
            self.put_trusted(&chr.blake3, data);
            Ok(())
        })
    }

    /// Streams a chunk's bytes to a writer (e.g. a socket or base64 encoder).
    pub fn write_chunk_to<W: Write>(&self, chr: &ChunkRef, out: &mut W) -> Result<(), String> {
        if let Some(data) = self.get(&chr.blake3) {
            out.write_all(&data)
                .map_err(|e| format!("chunk write: {e}"))
        } else {
            Err(format!("chunk {} missing", chr.blake3))
        }
    }

    /// Total bytes referenced by the store (for cache-pressure eviction).
    pub fn total_bytes(&self) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Ok(inner) = fs::read_dir(entry.path()) {
                        for f in inner.flatten() {
                            if let Ok(md) = f.metadata() {
                                total += md.len();
                            }
                        }
                    }
                } else if let Ok(md) = entry.metadata() {
                    total += md.len();
                }
            }
        }
        total
    }

    /// Enforces a maximum byte quota on the chunk cache by removing oldest chunks.
    pub fn enforce_cache_quota(&self, max_bytes: u64) -> Result<u64, String> {
        let current = self.total_bytes();
        if current <= max_bytes {
            return Ok(0);
        }

        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Ok(inner) = fs::read_dir(entry.path()) {
                        for f in inner.flatten() {
                            if let Ok(md) = f.metadata() {
                                if md.is_file() {
                                    let modified =
                                        md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                                    files.push((f.path(), md.len(), modified));
                                }
                            }
                        }
                    }
                } else if let Ok(md) = entry.metadata() {
                    if md.is_file() {
                        let modified = md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        files.push((entry.path(), md.len(), modified));
                    }
                }
            }
        }

        files.sort_by_key(|f| f.2);

        let mut reclaimed = 0u64;
        let mut remaining = current;
        for (path, size, _) in files {
            if remaining <= max_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                reclaimed += size;
                remaining = remaining.saturating_sub(size);
            }
        }

        if reclaimed > 0 {
            self.refresh_index();
        }

        Ok(reclaimed)
    }

    /// Removes all chunks (cache clear).
    pub fn clear(&self) {
        let _ = fs::remove_dir_all(&self.root);
        if let Ok(mut slot) = self.index.write() {
            *slot = None;
        }
        if let Ok(mut cache) = self.verified.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.manifest_cache.lock() {
            cache.map.clear();
            cache.order.clear();
        }
    }

    /// Default on-disk location used by peer-serving and bridge call sites.
    /// The FFI bridge pins this to a persistent app-data dir at startup; the
    /// temp-dir fallback is only for bare/core use and is wiped on reboot.
    pub fn default_root() -> PathBuf {
        DEFAULT_ROOT_OVERRIDE
            .get()
            .cloned()
            .or_else(|| std::env::var_os("SOSHAL_CHUNK_CACHE").map(PathBuf::from))
            .unwrap_or_else(|| std::env::temp_dir().join("soshal_chunks"))
    }

    /// Point `default_root()` at a persistent directory (the app data dir),
    /// called once at startup by the FFI bridge. Without this the temp-dir
    /// fallback dies on OS reboot and every stored blob (music, images,
    /// videos) becomes unreachable — "no device has this blob". No-op when
    /// already set; `SOSHAL_CHUNK_CACHE` still wins for operator overrides.
    pub fn set_default_root(dir: PathBuf) {
        let _ = DEFAULT_ROOT_OVERRIDE.set(dir);
    }

    /// Manifest persistence lives next to the chunk files so peer-serving
    /// (which has no DB access) can answer blob-range requests from disk alone.
    pub fn manifest_path(&self, blob_hash: &str) -> PathBuf {
        if !is_valid_hash(blob_hash) {
            let safe_hash: String = blob_hash
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            return self
                .root
                .join("invalid_manifests")
                .join(format!("{safe_hash}.json"));
        }
        self.root
            .join("manifests")
            .join(format!("{blob_hash}.json"))
    }

    pub fn save_manifest(&self, manifest: &ChunkManifest) -> Result<(), String> {
        if !is_valid_hash(&manifest.blob_hash) || !manifest.is_valid() {
            return Err("invalid manifest or blob_hash".to_string());
        }
        let path = self.manifest_path(&manifest.blob_hash);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("manifest dir: {e}"))?;
        }
        if let Ok(mut cache) = self.manifest_cache.lock() {
            cache.invalidate(&manifest.blob_hash);
        }
        let json = serde_json::to_string(manifest).map_err(|e| format!("manifest serde: {e}"))?;
        let mut tmp = path.with_extension("tmp");
        let mut n = 0u32;
        while tmp.exists() {
            n += 1;
            tmp = format!("{}.{n}", path.to_string_lossy()).into();
        }
        fs::write(&tmp, json)
            .and_then(|_| fs::rename(&tmp, &path))
            .map_err(|e| format!("manifest write: {e}"))?;
        if let Ok(mut slot) = self.index.write() {
            let idx = slot.get_or_insert_with(|| Arc::new(ChunkIndex::build(&self.root)));
            let idx = Arc::make_mut(idx);
            idx.ingest(manifest);
        }
        Ok(())
    }

    pub fn load_manifest(&self, blob_hash: &str) -> Option<ChunkManifest> {
        if !is_valid_hash(blob_hash) {
            return None;
        }
        if let Ok(cache) = self.manifest_cache.lock() {
            if let Some(m) = cache.get(blob_hash) {
                return Some(m.as_ref().clone());
            }
        }
        let raw = fs::read_to_string(self.manifest_path(blob_hash)).ok()?;
        let m: ChunkManifest = serde_json::from_str(&raw).ok()?;
        if m.blob_hash == blob_hash && m.is_valid() {
            if let Ok(mut cache) = self.manifest_cache.lock() {
                cache.put(blob_hash.to_string(), Arc::new(m.clone()));
            }
            Some(m)
        } else {
            None
        }
    }

    /// Finds the manifest of any stored blob that references `chunk_hash`.
    /// Peer swarm clients request by chunk hash; the CAS must resolve the
    /// owning blob to serve a range. Uses the in-memory index (built lazily
    /// from the manifests directory) so misses cost O(1), not a full scan.
    pub fn find_manifest_containing_chunk(&self, chunk_hash: &str) -> Option<ChunkManifest> {
        if chunk_hash.len() != 64 {
            return None;
        }
        let idx = self.index();
        if let Some(candidates) = idx.by_chunk.get(chunk_hash) {
            for (blob_hash, _) in candidates {
                if let Some(m) = self.load_manifest(blob_hash) {
                    return Some(m);
                }
            }
        }
        None
    }

    /// Returns the byte slice [offset, offset+len) of a stored blob, assembled
    /// from its chunks (which are verified on read). `None` if any chunk is
    /// missing or the manifest is invalid — never serves corrupt ranges.
    pub fn blob_slice(
        &self,
        manifest: &ChunkManifest,
        offset: usize,
        len: usize,
    ) -> Option<Vec<u8>> {
        if !manifest.is_valid() || len == 0 {
            return None;
        }
        let end = offset.checked_add(len)?;
        if end > manifest.total_size as usize {
            return None;
        }
        let mut out = Vec::with_capacity(len);
        let mut pos = offset;
        for c in &manifest.chunks {
            let c_start = c.offset as usize;
            let c_end = c_start + c.len;
            if c_end <= offset {
                continue;
            }
            if c_start >= end {
                break;
            }
            let mmap = self.get_mmap_cached(&c.blake3)?;
            let chunk = mmap.as_ref();
            let from = offset.saturating_sub(c_start);
            let to = (end - c_start).min(chunk.len());
            out.extend_from_slice(&chunk[from..to]);
            pos += to - from;
        }
        if pos < end {
            return None;
        }
        Some(out)
    }
}

/// Re-exposed for tests: re-read chunk data back out of the manifest.
pub fn manifest_bytes(store: &ChunkStore, manifest: &ChunkManifest) -> Vec<u8> {
    let mut out = Vec::with_capacity(manifest.total_size as usize);
    for c in &manifest.chunks {
        if let Some(d) = store.get(&c.blake3) {
            out.extend_from_slice(&d);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_roundtrip_and_dedup() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let data = vec![5u8; 1024];
        assert!(store.put(&data));
        assert!(!store.put(&data));
        assert_eq!(
            store.get(blake3::hash(&data).to_hex().as_str()).unwrap(),
            data
        );
    }

    #[test]
    fn put_rejects_mismatched_hash() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let data = vec![1u8; 32];
        assert!(!store.put_verified(&"00".repeat(32), &data));
    }

    #[test]
    fn store_file_reconstructs_exactly() {
        let root = soshal_test_util::tmp_root("cas");
        fs::create_dir_all(&root).unwrap();
        let store = ChunkStore::new(root.join("chunks"));
        let src = root.join("blob.bin");
        fs::write(
            &src,
            (0..4 * 1024 * 1024)
                .map(|i| (i % 253) as u8)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let manifest = store.store_file(&src).unwrap();
        assert!(manifest.total_size == 4 * 1024 * 1024);
        assert!(manifest.is_valid());
        assert_eq!(fs::read(&src).unwrap(), manifest_bytes(&store, &manifest));
        assert!(manifest.chunks.iter().all(|c| store.contains(&c.blake3)));
    }

    #[test]
    fn shared_region_dedups_across_blobs() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let shared: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let mut a = shared.clone();
        a.extend_from_slice(&[1u8; 1000]);
        let mut b = shared;
        b.extend_from_slice(&[2u8; 1000]);
        let ma = store.store_reader(std::io::Cursor::new(&a)).unwrap();
        let before = store.total_bytes();
        let mb = store.store_reader(std::io::Cursor::new(&b)).unwrap();
        let after = store.total_bytes();
        let overlap: usize = mb
            .chunks
            .iter()
            .filter(|c| ma.chunks.iter().any(|c2| c2.blake3 == c.blake3))
            .map(|c| c.len)
            .sum();
        assert!(
            overlap > 0,
            "content-defined chunking should re-find shared cut points"
        );
        let expected_new = b.len() as u64 - overlap as u64;
        assert!(after - before <= expected_new + 64 * 1024);
    }

    #[test]
    fn get_missing_returns_none() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        assert!(store.get(&"ab".repeat(32)).is_none());
    }

    #[test]
    fn get_mmap_zero_copy_read() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 253) as u8).collect();
        store.put(&data);
        let hash = blake3::hash(&data).to_hex().to_string();
        let map = store.get_mmap(&hash).expect("mapped chunk");
        assert_eq!(map.as_ref(), data.as_slice());
        let corrupt_hash = "aa".repeat(32);
        assert!(store.get_mmap(&corrupt_hash).is_none());
    }

    #[test]
    fn manifest_roundtrip_and_blob_slice_ranges() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let data: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();
        let loaded = store.load_manifest(&m.blob_hash).unwrap();
        assert_eq!(loaded, m);

        let full = store.blob_slice(&m, 0, data.len()).unwrap();
        assert_eq!(full, data);
        let mid = store.blob_slice(&m, 1000, 700 * 1024).unwrap();
        assert_eq!(mid, &data[1000..1000 + 700 * 1024]);
        let tail = store.blob_slice(&m, data.len() - 5, 5).unwrap();
        assert_eq!(tail, &data[data.len() - 5..]);
        assert!(store.blob_slice(&m, data.len() - 1, 2).is_none());
        assert!(store.blob_slice(&m, 0, 0).is_none());
    }

    #[test]
    fn load_manifest_rejects_corrupt() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let path = store.manifest_path(&"aa".repeat(32));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"blob_hash\":\"wrong\"").unwrap();
        assert!(store.load_manifest(&"aa".repeat(32)).is_none());
    }

    #[test]
    fn chunk_index_resolves_owner_manifest() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let data: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 249) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        let probe = &m.chunks[m.chunks.len() / 2];
        let owner = store.find_manifest_containing_chunk(&probe.blake3).unwrap();
        assert_eq!(owner.blob_hash, m.blob_hash);

        // Miss must not poison the index: a lookup with no hits returns None
        // and a later refresh restores full coverage.
        assert!(store
            .find_manifest_containing_chunk(&"ab".repeat(32))
            .is_none());

        store.clear();
        assert!(store
            .find_manifest_containing_chunk(&probe.blake3)
            .is_none());
    }

    #[test]
    fn index_refreshes_after_external_manifest_write() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 247) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();

        // Build the index before the manifest appears on disk (as if the
        // store was created while the manifest was absent).
        assert!(store
            .find_manifest_containing_chunk(&m.chunks[0].blake3)
            .is_none());

        // Simulate another process writing the manifest directly.
        let mpath = store.manifest_path(&m.blob_hash);
        fs::create_dir_all(mpath.parent().unwrap()).unwrap();
        fs::write(mpath, serde_json::to_string(&m).unwrap()).unwrap();

        // Stale index still misses...
        assert!(store
            .find_manifest_containing_chunk(&m.chunks[0].blake3)
            .is_none());
        // ...until explicitly refreshed.
        store.refresh_index();
        assert!(store
            .find_manifest_containing_chunk(&m.chunks[0].blake3)
            .is_some());
    }

    #[test]
    fn mmap_bad_len_and_verified_cache_hit() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        assert!(store.get_mmap("abc").is_none());

        let data: Vec<u8> = (0..512).map(|i| (i % 251) as u8).collect();
        store.put(&data);
        let hash = blake3::hash(&data).to_hex().to_string();
        let first = store.get_mmap(&hash).expect("first read");
        assert_eq!(first.as_ref(), data.as_slice());
        // Second read must skip the re-hash via the verified cache.
        let second = store.get_mmap(&hash).expect("verified-cache hit");
        assert_eq!(second.as_ref(), data.as_slice());
    }

    #[test]
    fn verified_cache_clears_when_full() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));
        let hashes: Vec<String> = (0..300)
            .map(|i| {
                store.put(&[i as u8]);
                blake3::hash(&[i as u8]).to_hex().to_string()
            })
            .collect();
        for h in &hashes {
            assert!(store.get_mmap(h).is_some());
        }
        // Cap is 256; the 257th insert evicts the oldest entry.
        assert_eq!(store.verified.lock().unwrap().len(), 256);
        // Evicted entry re-reads fine via the re-hash path.
        assert!(store.get_mmap(&hashes[0]).is_some());
    }

    #[test]
    fn manifest_save_tmp_collision_and_load_edge_cases() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));

        // blob_hash len != 64 -> None.
        assert!(store.load_manifest("abc").is_none());

        // Valid JSON whose blob_hash mismatches the key -> None.
        let x = "aa".repeat(32);
        let y = "bb".repeat(32);
        let px = store.manifest_path(&x);
        fs::create_dir_all(px.parent().unwrap()).unwrap();
        fs::write(
            &px,
            format!(r#"{{"blob_hash":"{y}","total_size":0,"chunks":[]}}"#),
        )
        .unwrap();
        assert!(store.load_manifest(&x).is_none());

        // Pre-existing .tmp forces the retry loop; original untouched.
        let data: Vec<u8> = (0..70 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        let p = store.manifest_path(&m.blob_hash);
        let collision = p.with_extension("tmp");
        fs::create_dir_all(collision.parent().unwrap()).unwrap();
        fs::write(&collision, "stale").unwrap();
        store.save_manifest(&m).unwrap();
        assert!(p.is_file());
        assert_eq!(fs::read_to_string(&collision).unwrap(), "stale");
        assert_eq!(store.load_manifest(&m.blob_hash).unwrap(), m);

        // Cache-hit fast path: manifest file deleted, load still succeeds.
        fs::remove_file(&p).unwrap();
        assert_eq!(store.load_manifest(&m.blob_hash).unwrap(), m);
    }

    #[test]
    fn manifest_cache_fifo_and_index_build_skip_invalid() {
        // FifoCache FIFO cap-64 eviction.
        let mut cache = FifoCache::<ChunkManifest> {
            cap: 64,
            ..Default::default()
        };
        let mk = |i: usize| ChunkManifest {
            blob_hash: format!("{i:02}") + &"ab".repeat(31),
            total_size: 0,
            chunks: vec![],
        };
        for i in 0..65 {
            cache.put(format!("k{i}"), Arc::new(mk(i)));
        }
        assert_eq!(cache.map.len(), 64);
        assert!(cache.get("k0").is_none());
        assert!(cache.get("k1").is_some());
        assert!(cache.get("k64").is_some());

        // ChunkIndex::build skips garbage and structurally invalid manifests.
        let root = soshal_test_util::tmp_root("cas");
        let md = root.join("manifests");
        fs::create_dir_all(&md).unwrap();
        fs::write(md.join("garbage.json"), "not json").unwrap();
        fs::write(
            md.join("bad.json"),
            format!(
                r#"{{"blob_hash":"{}","total_size":10,"chunks":[]}}"#,
                "aa".repeat(32)
            ),
        )
        .unwrap();
        let good = ChunkManifest {
            blob_hash: "cd".repeat(32),
            total_size: 5,
            chunks: vec![ChunkRef {
                blake3: "ab".repeat(32),
                offset: 0,
                len: 5,
            }],
        };
        fs::write(md.join("good.json"), serde_json::to_string(&good).unwrap()).unwrap();
        let idx = ChunkIndex::build(&root);
        assert_eq!(idx.by_chunk.len(), 1);
        assert_eq!(
            idx.by_chunk.get(&"ab".repeat(32)),
            Some(&vec![("cd".repeat(32), 0)])
        );
    }

    #[test]
    fn blob_slice_overflow_and_store_edges() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas"));

        // Offset overflow -> None.
        let empty = ChunkManifest {
            blob_hash: "aa".repeat(32),
            total_size: 0,
            chunks: vec![],
        };
        assert!(store.blob_slice(&empty, usize::MAX, 1).is_none());
        assert!(store.blob_slice(&empty, usize::MAX, 0).is_none());

        // Missing/corrupt chunk -> None (short-circuits via get_mmap). The
        // pos < end branch is unreachable for valid manifests: every chunk
        // file that passes hash verification matches its key, so it covers
        // exactly c.len bytes; a shorter file fails the hash first.
        let data: Vec<u8> = (0..70 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(store.chunk_path(&m.chunks[0].blake3))
            .unwrap()
            .set_len(10)
            .unwrap();
        assert!(store.blob_slice(&m, 0, data.len()).is_none());

        // total_bytes counts root-level plain files plus chunk subdirs.
        let root2 = soshal_test_util::tmp_root("cas");
        let store2 = ChunkStore::new(root2.clone());
        fs::write(root2.join("plain.bin"), vec![7u8; 100]).unwrap();
        store2.put(&[9u8; 200]);
        assert_eq!(store2.total_bytes(), 300);

        // find_manifest_containing_chunk: bad len -> None; deleted stale
        // candidate file -> loop continues to the surviving manifest.
        assert!(store.find_manifest_containing_chunk("abc").is_none());
        let root3 = soshal_test_util::tmp_root("cas");
        let store3 = ChunkStore::new(root3);
        let m3 = store3.store_reader(std::io::Cursor::new(&data)).unwrap();
        store3.save_manifest(&m3).unwrap();
        let mut m4 = m3.clone();
        m4.blob_hash = "ef".repeat(32);
        let p4 = store3.manifest_path(&m4.blob_hash);
        fs::create_dir_all(p4.parent().unwrap()).unwrap();
        fs::write(&p4, serde_json::to_string(&m4).unwrap()).unwrap();
        store3.refresh_index();
        fs::remove_file(store3.manifest_path(&m3.blob_hash)).unwrap();
        let owner = store3
            .find_manifest_containing_chunk(&m3.chunks[0].blake3)
            .unwrap();
        assert_eq!(owner.blob_hash, m4.blob_hash);

        // store_reader over an empty reader -> zero-chunk valid manifest.
        let m5 = store
            .store_reader(std::io::Cursor::new(Vec::<u8>::new()))
            .unwrap();
        assert!(m5.chunks.is_empty());
        assert_eq!(m5.total_size, 0);
        assert!(m5.is_valid());
    }

    #[test]
    fn test_enforce_cache_quota() {
        let root = soshal_test_util::tmp_root("cas_quota");
        let store = ChunkStore::new(root);
        let chunk1 = vec![1u8; 1024];
        let chunk2 = vec![2u8; 1024];
        let chunk3 = vec![3u8; 1024];
        store.put(&chunk1);
        store.put(&chunk2);
        store.put(&chunk3);
        assert!(store.total_bytes() >= 3072);

        // Enforce quota of 2000 bytes -> should prune oldest chunks
        let reclaimed = store.enforce_cache_quota(2000).unwrap();
        assert!(reclaimed > 0);
        assert!(store.total_bytes() <= 2000);
    }

    #[test]
    fn test_cas_hash_validation_and_path_traversal() {
        let root = soshal_test_util::tmp_root("cas_security");
        let store = ChunkStore::new(root);

        // Short or empty hashes must not panic and must return false / None
        assert!(!store.contains(""));
        assert!(!store.contains("a"));
        assert!(!store.contains("ab"));
        assert!(store.get("").is_none());
        assert!(store.get("invalid_hash").is_none());

        // Path traversal attempts must be rejected
        assert!(!store.put_trusted("../../../evil", b"payload"));
        assert!(!store.contains("../../../evil"));

        // Invalid manifest hash must fail to save
        let manifest = ChunkManifest {
            blob_hash: "../../../evil".to_string(),
            total_size: 10,
            chunks: vec![],
        };
        assert!(store.save_manifest(&manifest).is_err());

        // Valid hash works
        let data = b"valid chunk content";
        let valid_hash = blake3::hash(data).to_hex().to_string();
        assert!(store.put(data));
        assert!(store.contains(&valid_hash));
        assert_eq!(store.get(&valid_hash).unwrap(), data);
    }
}
