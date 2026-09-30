//! Local content-addressed chunk store (CAS).
//!
//! Chunks live at `<root>/<first2>/<rest>.chunk` keyed by BLAKE3 hex. Writes
//! are idempotent: `put` verifies the hash matches the key before writing, so
//! a corrupted or malicious chunk can never poison a valid hash slot. Files
//! are never overwritten — a hash that exists is trusted as-is.
//!
//! Dedup is implicit: five posts sharing one audio clip produce five identical
//! chunk hashes, all resolving to a single on-disk file.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use rayon::prelude::*;

use crate::chunking::{store_reader_with_data, ChunkManifest, ChunkRef};

/// Overlapping-chunk count at which `blob_slice` switches from a serial map to
/// a rayon one. Below this the thread hand-off costs more than the disk-backed
/// mmap lookups it would overlap.
const PARALLEL_SLICE_CHUNKS: usize = 4;

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
    /// Verified (hash, size, mtime) entries; every read re-hashes regardless,
    /// the cache only skips the redundant mark_verified write.
    verified: Arc<Mutex<VerifiedChunks>>,
    /// Manifest LRU: peer-serving re-reads manifests/*.json per chunk request
    /// (both TCP and QUIC bulk paths). Capped FIFO, invalidated on save/clear.
    manifest_cache: Arc<Mutex<FifoCache<ChunkManifest>>>,
    /// Open-chunk LRU: `blob_slice` maps each chunk on demand; caching the
    /// maps (page-cache backed) avoids a File::open + mmap syscall pair per
    /// chunk on every range read. Capped FIFO like the manifest cache.
    mmap_cache: Arc<Mutex<FifoCache<memmap2::Mmap>>>,
}

/// LRU-capped cache of Arc'd values keyed by hash string with O(log C) generational eviction.
struct FifoCache<T> {
    order: BTreeSet<(u64, String)>,
    map: HashMap<String, (u64, Arc<T>)>,
    cap: usize,
    next_epoch: u64,
}

impl<T> Default for FifoCache<T> {
    fn default() -> Self {
        Self {
            order: BTreeSet::new(),
            map: HashMap::new(),
            cap: 0,
            next_epoch: 0,
        }
    }
}

impl<T> FifoCache<T> {
    fn get(&mut self, key: &str) -> Option<Arc<T>> {
        let (epoch, val) = self.map.get_mut(key)?;
        let old_epoch = *epoch;
        self.next_epoch += 1;
        *epoch = self.next_epoch;
        self.order.remove(&(old_epoch, key.to_string()));
        self.order.insert((self.next_epoch, key.to_string()));
        Some(val.clone())
    }

    fn put(&mut self, key: String, value: Arc<T>) {
        self.next_epoch += 1;
        if let Some((epoch, old_val)) = self.map.get_mut(&key) {
            let old_epoch = *epoch;
            *epoch = self.next_epoch;
            *old_val = value;
            self.order.remove(&(old_epoch, key.clone()));
            self.order.insert((self.next_epoch, key));
            return;
        }
        if self.cap > 0 && self.map.len() >= self.cap {
            if let Some((_, oldest)) = self.order.pop_first() {
                self.map.remove(&oldest);
            }
        }
        self.order.insert((self.next_epoch, key.clone()));
        self.map.insert(key, (self.next_epoch, value));
    }

    fn invalidate(&mut self, blob_hash: &str) {
        if let Some((epoch, _)) = self.map.remove(blob_hash) {
            self.order.remove(&(epoch, blob_hash.to_string()));
        }
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
        let mut slot = self.index.write().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(built.clone());
            built
        } else {
            slot.as_ref().cloned().unwrap_or(built)
        }
    }

    /// Rebuild the index from scratch (used when on-disk state changed
    /// outside this store, e.g. cache eviction ran).
    pub fn refresh_index(&self) {
        let mut slot = self.index.write().unwrap_or_else(|e| e.into_inner());
        *slot = Some(Arc::new(ChunkIndex::build(&self.root)));
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
            let mut p = PathBuf::with_capacity(self.root.as_os_str().len() + safe_hash.len() + 24);
            p.push(&self.root);
            p.push("invalid");
            p.push(prefix);
            p.push(format!("{safe_hash}.chunk"));
            return p;
        }
        let hash_lower = hash.to_ascii_lowercase();
        let (a, b) = hash_lower.split_at(2);
        let mut p = PathBuf::with_capacity(self.root.as_os_str().len() + hash_lower.len() + 10);
        p.push(&self.root);
        p.push(a);
        p.push(format!("{b}.chunk"));
        p
    }

    pub fn contains(&self, hash: &str) -> bool {
        if !is_valid_hash(hash) {
            return false;
        }
        self.chunk_path(hash).is_file()
    }

    /// Reads a chunk and verifies its BLAKE3 hash. `None` on missing/corrupt.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        if !is_valid_hash(hash) {
            return None;
        }
        let path = self.chunk_path(hash);
        let file = fs::File::open(&path).ok()?;
        let md = file.metadata().ok()?;
        if !md.is_file() {
            return None;
        }
        if md.len() == 0 {
            if blake3::hash(&[]).to_hex().as_str() == hash {
                return Some(Vec::new());
            }
            return None;
        }
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
        if !md.is_file() {
            return None;
        }
        let size = md.len();
        if size == 0 {
            return None;
        }
        let mtime = md.modified().ok();
        // Safety: read-only mapping of a file we opened read-only; the Mmap is
        // the sole handle to the region and unmap happens on drop.
        #[allow(unsafe_code)]
        let map = unsafe { memmap2::Mmap::map(&file).ok()? };
        // Always re-hash the mapped bytes before trusting a cached (size,mtime)
        // verdict: on coarse-mtime filesystems a chunk replaced within the
        // same tick and same byte length would otherwise return stale/corrupt
        // content under the old verified verdict. The verified cache only
        // skips the redundant mark_verified write, never the hash.
        if !blake3::hash(map.as_ref())
            .to_hex()
            .as_str()
            .eq_ignore_ascii_case(hash)
        {
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
        if let Ok(mut cache) = self.mmap_cache.lock() {
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

    fn mark_verified(&self, hash: String, size: u64, mtime: Option<SystemTime>) {
        if let Ok(mut cache) = self.verified.lock() {
            if !cache.contains_key(&hash) && cache.len() >= 256 {
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
        if expected.len() != 64
            || !expected.eq_ignore_ascii_case(blake3::hash(data).to_hex().as_str())
        {
            return false;
        }
        self.put_trusted(expected, data)
    }

    /// Chunks a file on disk and stores every chunk, deduplicating as it goes.
    /// Returns the manifest (empty-file safe).
    pub fn store_file(&self, path: &Path) -> Result<ChunkManifest, String> {
        let meta = fs::metadata(path).map_err(|e| format!("metadata {path:?}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("path is not a regular file: {path:?}"));
        }
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
    ///
    /// Reads from the mapping rather than through [`ChunkStore::get`]. `get`
    /// is `get_mmap(hash).map(|m| m.as_ref().to_vec())`, so streaming a chunk
    /// used to allocate and fill a heap copy of the whole thing — up to 16 MiB
    /// — purely to hand it to `write_all` and drop it. The mapping is
    /// hash-verified on the way in either way; the only thing removed is the
    /// copy. Going through `get_mmap_cached` also lets a repeated stream of the
    /// same chunk skip the open+mmap.
    pub fn write_chunk_to<W: Write>(&self, chr: &ChunkRef, out: &mut W) -> Result<(), String> {
        match self.get_mmap_cached(&chr.blake3) {
            Some(mmap) => out
                .write_all(mmap.as_ref())
                .map_err(|e| format!("chunk write: {e}")),
            None => Err(format!("chunk {} missing", chr.blake3)),
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

    /// One walk of the store, yielding every file with its size and mtime.
    ///
    /// `chunk_path` puts chunks in `root/<2-char prefix>/<hash>` and manifests in
    /// `root/manifests/<hash>.json`, so the store is exactly two levels deep and
    /// this sees every file in it.
    ///
    /// Uses `DirEntry::file_type`/`metadata` rather than `path().is_dir()` plus
    /// `Path::metadata()`: the first comes from the directory entry the kernel
    /// already returned, the second resolves the name once, so a file costs one
    /// stat instead of two. That matters because this is the store's only
    /// full-tree scan and it runs over every chunk.
    fn walk_store(&self) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
        let Ok(entries) = fs::read_dir(&self.root) else {
            return files;
        };
        let mut push = |entry: &fs::DirEntry| {
            // `file_type` avoids a stat on the common case; fall back to
            // `metadata` if the filesystem will not answer it from `d_type`.
            let is_file = match entry.file_type() {
                Ok(ft) => ft.is_file(),
                Err(_) => entry.metadata().map(|md| md.is_file()).unwrap_or(false),
            };
            if !is_file {
                return;
            }
            let Ok(md) = entry.metadata() else { return };
            let modified = md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            files.push((entry.path(), md.len(), modified));
        };
        for entry in entries.flatten() {
            // Only descend one level: the layout is two levels, and a deeper
            // tree would be someone else's data in our cache directory.
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Ok(inner) = fs::read_dir(entry.path()) {
                    for f in inner.flatten() {
                        push(&f);
                    }
                }
            } else {
                push(&entry);
            }
        }
        files
    }

    /// Enforces a maximum byte quota on the chunk cache by removing oldest chunks.
    ///
    /// One walk, not two. This used to call `total_bytes()` — a full recursive
    /// scan — and then, only if over quota, walk the *same* tree a second time to
    /// build the eviction list. On a phone CAS with 50k chunks that is two full
    /// scans of the directory, every time, for a number the first scan already
    /// had. Both walks also used `path().is_dir()` alongside `Path::metadata()`,
    /// so each file cost two stats.
    ///
    /// The `current <= max_bytes` early return is preserved, and now costs the
    /// single walk instead of a scan plus a discarded second one. An incremental
    /// byte counter maintained by `put_trusted`/`remove` would make this O(1),
    /// but it would have to survive `clear`, external writes and the fact that
    /// the store is process-global with a `default_root` that can be re-pointed
    /// by the bridge at init — too much state to get quietly wrong for a
    /// cache-pressure heuristic.
    pub fn enforce_cache_quota(&self, max_bytes: u64) -> Result<u64, String> {
        let mut files = self.walk_store();
        let current: u64 = files.iter().map(|f| f.1).sum();
        if current <= max_bytes {
            return Ok(0);
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
        if let Ok(mut cache) = self.manifest_cache.lock() {
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

    /// The bytes for `[offset, offset+len)` **when the whole range lies inside
    /// one chunk**, as the mapped chunk plus the sub-range to use.
    ///
    /// `None` for a cross-chunk range, a range outside the manifest, or a chunk
    /// that is missing or fails its hash — so a caller falls back to
    /// [`ChunkStore::blob_slice`], which is always correct and always copies.
    ///
    /// This exists so a serving path can stream straight from the mapping.
    /// [`ChunkStore::blob_slice`] has to build a `Vec` of the whole range
    /// first, so a single-chunk read (the overwhelmingly common case for a
    /// media chunk) pays an allocation of up to `MAX_CHUNK` bytes plus a full
    /// copy before the transport copies it again. Here the transport's copy is
    /// the only one.
    ///
    /// The mapping is hash-verified by `get_mmap_cached` before it is handed
    /// out, so a caller that streams this slice is streaming verified bytes —
    /// the same guarantee the TCP zero-copy path depends on.
    pub fn blob_range_mapped(
        &self,
        manifest: &ChunkManifest,
        offset: usize,
        len: usize,
    ) -> Option<(Arc<memmap2::Mmap>, std::ops::Range<usize>)> {
        if !manifest.is_valid() || len == 0 {
            return None;
        }
        let end = offset.checked_add(len)?;
        if end > manifest.total_size as usize {
            return None;
        }
        let chunk = manifest.chunks.iter().find(|c| {
            let c_start = c.offset as usize;
            offset >= c_start && end <= c_start + c.len
        })?;
        let mmap = self.get_mmap_cached(&chunk.blake3)?;
        let c_start = chunk.offset as usize;
        // A stored chunk shorter than the manifest declares cannot serve the
        // requested window; `get_mmap_cached` already rejects one that fails
        // its hash, but a manifest that over-declares its own length is still
        // checked here rather than trusted.
        let to = (end - c_start).min(mmap.len());
        let from = offset - c_start;
        if from > to {
            return None;
        }
        Some((mmap, from..to))
    }

    /// Returns the byte slice [offset, offset+len) of a stored blob, assembled
    /// from its chunks (which are verified on read). `None` if any chunk is
    /// missing or the manifest is invalid — never serves corrupt ranges.
    ///
    /// `is_valid()` guarantees the manifest's chunks tile `[0, total_size)`
    /// contiguously and without overlap, so a complete assembly is always
    /// exactly `len` bytes. Corruption — a stored chunk that does not match its
    /// own BLAKE3, including one truncated on disk — is rejected by
    /// [`ChunkStore::get_mmap`], which re-hashes the mapped bytes on every read
    /// before trusting them; that, not any length arithmetic here, is what
    /// refuses a short stored chunk.
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
        // The chunks overlapping [offset, end) are identified serially (pure
        // arithmetic over the manifest), then mapped in parallel. Each map is
        // independent — `get_mmap_cached` is `&self` and guards its own caches
        // — and a large range read otherwise paid a serialized disk-backed
        // mmap fault per chunk. Result order is preserved by collecting into a
        // `Vec` in manifest order, so the assembled bytes are byte-identical.
        let overlapping: Vec<&ChunkRef> = manifest
            .chunks
            .iter()
            .take_while(|c| (c.offset as usize) < end)
            .filter(|c| c.offset as usize + c.len > offset)
            .collect();
        if overlapping.is_empty() {
            return None;
        }
        // Resolve every chunk's source window first, so the output buffer is
        // written exactly once per byte.
        //
        // This used to build each chunk's slice as its own `Vec` and then
        // `extend_from_slice` it into the output: two copies of every byte, and
        // a peak of 2x the range on top of the result, because all the
        // per-chunk `Vec`s were alive at once under the rayon collect. The
        // windows are pure arithmetic once the mmap is in hand, so resolving
        // them and copying them are separable — and only the second is the
        // expensive one.
        let resolve = |c: &ChunkRef| -> Option<SourceWindow> {
            let mmap = self.get_mmap_cached(&c.blake3)?;
            let chunk = mmap.as_ref();
            let c_start = c.offset as usize;
            let c_end = c_start + c.len;
            // `overlapping` is pre-filtered, but re-check the bounds so a
            // manifest that lies about its own geometry still cannot panic the
            // slicing. A chunk that contributes nothing is a legal empty
            // window, not a failure.
            if c_end <= offset || c_start >= end {
                return Some(SourceWindow {
                    mmap,
                    from: 0,
                    to: 0,
                });
            }
            let from = offset.saturating_sub(c_start);
            let to = (end - c_start).min(chunk.len());
            if from > to {
                return None;
            }
            Some(SourceWindow { mmap, from, to })
        };
        let windows: Vec<Option<SourceWindow>> = if overlapping.len() >= PARALLEL_SLICE_CHUNKS {
            overlapping.par_iter().map(|c| resolve(c)).collect()
        } else {
            overlapping.iter().map(|c| resolve(c)).collect()
        };
        let mut out = vec![0u8; len];
        // The windows must tile [0, len) exactly, and no chunk may be missing —
        // the documented contract is `None` if any chunk is missing.
        //
        // Note this is a *panic guard*, not the corruption gate. The old code's
        // `out.len() != len` check sounded like the thing that refused a short
        // stored chunk, and it was not: `get_mmap` re-hashes the mapped bytes
        // and rejects the truncated chunk before any of this arithmetic runs.
        // Verified by removing the per-mmap hash check and this check together,
        // which is what actually makes `blob_slice_refuses_short_stored_chunk`
        // fail. What this check buys is that `split_at_mut` below cannot panic
        // in a media-serving path if the window geometry is ever wrong, and it
        // keeps the documented "missing chunk is `None`" contract explicit.
        let windows: Vec<SourceWindow> = windows.into_iter().collect::<Option<Vec<_>>>()?;
        if windows.iter().map(|w| w.to - w.from).sum::<usize>() != len {
            return None;
        }
        // Hand the parallel copy disjoint destination segments. Splitting is
        // pure pointer arithmetic, so doing it serially up front costs nothing
        // and removes the need for the copy to coordinate at all.
        let mut rest: &mut [u8] = &mut out;
        let mut dsts: Vec<&mut [u8]> = Vec::with_capacity(windows.len());
        for w in &windows {
            let n = w.to - w.from;
            let (head, tail) = rest.split_at_mut(n);
            dsts.push(head);
            rest = tail;
        }
        let copy = |(dst, w): (&mut &mut [u8], &SourceWindow)| {
            dst.copy_from_slice(&w.mmap[w.from..w.to]);
        };
        if dsts.len() >= PARALLEL_SLICE_CHUNKS {
            dsts.par_iter_mut().zip(&windows).for_each(copy);
        } else {
            dsts.iter_mut().zip(&windows).for_each(copy);
        }
        Some(out)
    }
}

/// One chunk's contribution to a range read: a mapped chunk plus the window
/// within it that the range covers. Resolved before any copying happens, so the
/// output can be sized and written once.
struct SourceWindow {
    mmap: Arc<memmap2::Mmap>,
    from: usize,
    to: usize,
}

/// Re-exposed for tests: re-read chunk data back out of the manifest.
///
/// Extends straight from each chunk's mapping. The old shape called
/// `store.get(&c.blake3)`, which copies the chunk into a fresh `Vec` that is
/// then `extend_from_slice`d into a second, full-blob buffer — so a whole-blob
/// read peaked at 2x the blob, with the per-chunk copy contributing nothing the
/// mapping did not already have.
pub fn manifest_bytes(store: &ChunkStore, manifest: &ChunkManifest) -> Vec<u8> {
    let mut out = Vec::with_capacity(manifest.total_size as usize);
    for c in &manifest.chunks {
        if let Some(mmap) = store.get_mmap_cached(&c.blake3) {
            out.extend_from_slice(mmap.as_ref());
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

    /// The rayon path must be reached for a multi-chunk range *and* return
    /// exactly the requested bytes. The first assertion guards against the
    /// threshold drifting above the chunk count of a realistic blob, which
    /// would leave the parallel branch silently untested.
    #[test]
    fn blob_slice_parallel_path_returns_exact_range() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas-parallel"));
        // Pseudo-random bytes so FastCDC finds its natural boundaries instead of
        // running to MAX_CHUNK on every segment (a low-entropy buffer produces
        // few, huge chunks and would not reach the rayon threshold).
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let data: Vec<u8> = (0..12 * 1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        assert!(
            m.chunks.len() >= PARALLEL_SLICE_CHUNKS,
            "fixture must have >= {PARALLEL_SLICE_CHUNKS} chunks to reach the rayon branch, got {}",
            m.chunks.len()
        );
        for (offset, len) in [
            (0usize, data.len()),
            (1000, 700 * 1024),
            (0, PARALLEL_SLICE_CHUNKS * 4096),
            (5_000_000, 1_000_000),
            (data.len() - 1, 1),
        ] {
            let got = store
                .blob_slice(&m, offset, len)
                .unwrap_or_else(|| panic!("range {offset}+{len} should resolve"));
            assert_eq!(got.len(), len, "range {offset}+{len} wrong length");
            assert_eq!(
                got,
                &data[offset..offset + len],
                "range {offset}+{len} wrong bytes"
            );
        }
    }

    /// A manifest is rejected by `is_valid()` unless its chunks tile
    /// `[0, total_size)` contiguously, so the only reachable short-assembly is
    /// a chunk whose stored bytes are shorter than declared. That must surface
    /// as `None`, never as a truncated buffer.
    #[test]
    fn blob_slice_refuses_short_stored_chunk() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas-short"));
        let data: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
        let honest = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        // Truncate the last chunk on disk while the manifest keeps declaring
        // its full length, and bump total_size so `is_valid()` still passes.
        let last = honest.chunks.last().expect("at least one chunk");
        let path = store.chunk_path(&last.blake3);
        let mut on_disk = fs::read(&path).unwrap();
        on_disk.truncate(on_disk.len() - 1);
        fs::write(&path, &on_disk).unwrap();
        assert!(
            store.blob_slice(&honest, 0, data.len()).is_none(),
            "a short stored chunk must be refused, not served truncated"
        );
    }

    /// The mapped-range fast path must be byte-identical to `blob_slice` for
    /// every range it claims, and must decline everything it cannot serve — the
    /// QUIC server treats a decline as "fall back", so a false accept would
    /// return wrong bytes and a false decline would just lose the optimization.
    #[test]
    fn blob_range_mapped_agrees_with_blob_slice_and_declines_the_rest() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas-mapped"));
        let mut state = 0xDEAD_BEEF_1234_5678u64;
        let data: Vec<u8> = (0..6 * 1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        assert!(m.chunks.len() >= 2, "fixture must span several chunks");

        let mut accepted = 0usize;
        let mut declined = 0usize;
        // Walk ranges on a fine grid so the single-chunk and cross-chunk cases
        // are both hit, and the chunk boundaries are straddled.
        for offset in (0..data.len()).step_by(997) {
            for len in [1usize, 333, 4096, 200_000] {
                let end = (offset + len).min(data.len());
                if end == offset {
                    continue;
                }
                let want = &data[offset..end];
                match store.blob_range_mapped(&m, offset, end - offset) {
                    Some((mmap, range)) => {
                        accepted += 1;
                        assert_eq!(
                            &mmap[range],
                            want,
                            "mapped bytes must match the blob at {offset}+{}",
                            end - offset
                        );
                        // Anything it accepts must also be what blob_slice says.
                        assert_eq!(
                            store.blob_slice(&m, offset, end - offset).as_deref(),
                            Some(want)
                        );
                    }
                    None => {
                        declined += 1;
                    }
                }
            }
        }
        assert!(accepted > 0, "the grid must hit some single-chunk ranges");
        assert!(
            declined > 0,
            "the grid must hit some cross-chunk ranges to decline"
        );

        // Ranges it must always decline.
        assert!(
            store.blob_range_mapped(&m, 0, 0).is_none(),
            "an empty range is not a range"
        );
        assert!(
            store.blob_range_mapped(&m, data.len() - 1, 2).is_none(),
            "a range past the end must be declined"
        );
        // A whole-blob request spans chunks, so it is never mappable.
        assert!(store.blob_range_mapped(&m, 0, data.len()).is_none());
        // A hash that is not in the store at all.
        let missing = ChunkManifest {
            blob_hash: "e".repeat(64),
            total_size: 16,
            chunks: vec![ChunkRef {
                blake3: "f".repeat(64),
                offset: 0,
                len: 16,
            }],
        };
        assert!(store.blob_range_mapped(&missing, 0, 16).is_none());
    }

    /// The mapped range must not hand out bytes from a chunk that fails its
    /// hash. It gets that guarantee from the same verification `blob_slice` uses,
    /// but the fast path bypasses `blob_slice`, so it needs its own pin — a
    /// corrupt chunk streamed from the mapping is exactly what the hash exists to
    /// prevent.
    ///
    /// **Known limit, and it is not this function's:** `get_mmap_cached` returns
    /// the cached mapping on a hit without re-hashing, so a chunk corrupted
    /// *after* it was first mapped is served stale. This was found by writing
    /// this test, and it is pre-existing and identical on the `blob_slice` path
    /// — `blob_slice` on a warmed, corrupted chunk returns bytes too (verified:
    /// 1024 bytes for a 1024-byte request). Low severity: chunk files are named
    /// by their own hash, so nothing in this codebase writes different content
    /// to a chunk path, and the CAS lives in the app's own cache directory, so
    /// the realistic causes are bit rot or disk corruption rather than a hostile
    /// writer. Closing it means re-hashing on every cache hit, which is the
    /// cost the cache exists to avoid — a deliberate trade, not an oversight, but
    /// one that should be a conscious decision rather than a documented-in-a-test
    /// accident.
    #[test]
    fn blob_range_mapped_refuses_an_unverified_chunk() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas-mapped-bad"));
        let data: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        let first = &m.chunks[0];
        let path = store.chunk_path(&first.blake3);
        let mut on_disk = fs::read(&path).unwrap();
        on_disk[0] ^= 0xFF;
        fs::write(&path, &on_disk).unwrap();
        assert!(
            store
                .blob_range_mapped(&m, first.offset as usize, first.len)
                .is_none(),
            "a chunk that no longer matches its hash must not be served"
        );
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

        // LRU verification: accessing k1 moves it to MRU, so adding k65 evicts k2 instead of k1.
        let _ = cache.get("k1");
        cache.put("k65".to_string(), Arc::new(mk(65)));
        assert_eq!(cache.map.len(), 64);
        assert!(
            cache.get("k1").is_some(),
            "recently accessed k1 must be preserved"
        );
        assert!(cache.get("k2").is_none(), "unaccessed k2 must be evicted");
        assert!(cache.get("k65").is_some());

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

    /// Quota enforcement is now one walk that builds the eviction list and the
    /// total at the same time, so the *order* of that list is the part worth
    /// pinning: eviction must be oldest-first, and the newest chunk must be the
    /// one that survives.
    ///
    /// The mtimes are set explicitly because three chunks written back to back
    /// can share a filesystem timestamp, and then eviction order is just
    /// directory order — the test would pass or fail for reasons unrelated to
    /// the code.
    #[test]
    fn quota_evicts_oldest_first_and_leaves_the_newest() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas_quota_lru"));
        // Distinct contents so a surviving chunk is identifiable by its bytes.
        let chunks: Vec<Vec<u8>> = (0..5u8).map(|i| vec![i.wrapping_add(1); 1024]).collect();
        let hashes: Vec<String> = chunks
            .iter()
            .map(|c| {
                let h = blake3::hash(c).to_hex().to_string();
                assert!(store.put_trusted(&h, c), "chunk must be stored");
                h
            })
            .collect();
        assert_eq!(hashes.len(), 5);

        // Oldest first, with a wide gap so no two share a timestamp.
        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        for (i, h) in hashes.iter().enumerate() {
            let f = fs::File::options()
                .write(true)
                .open(store.chunk_path(h))
                .expect("open chunk to stamp it");
            f.set_times(
                std::fs::FileTimes::new()
                    .set_modified(base + std::time::Duration::from_secs(i as u64 * 3600)),
            )
            .expect("set mtime");
        }

        // A quota that leaves room for two chunks forces two evictions, so a
        // "sorted the wrong way" bug cannot hide behind evicting everything.
        let chunk_len = 1024usize;
        let chunk_bytes = chunk_len as u64;
        let per_chunk_total = chunk_bytes * 5;
        let reclaimed = store
            .enforce_cache_quota(per_chunk_total - chunk_bytes * 2)
            .unwrap();
        assert!(
            reclaimed > 0,
            "over-quota store must reclaim something, store is {per_chunk_total}"
        );

        // The two oldest are gone; the newest three are still readable and
        // still hold their own bytes.
        for (i, h) in hashes.iter().enumerate() {
            let present = store
                .blob_range_mapped(
                    &ChunkManifest {
                        blob_hash: "a".repeat(64),
                        total_size: chunk_bytes,
                        chunks: vec![ChunkRef {
                            blake3: h.clone(),
                            offset: 0,
                            len: chunk_len,
                        }],
                    },
                    0,
                    chunk_len,
                )
                .is_some();
            assert_eq!(
                present,
                i >= 2,
                "chunk {i} ({}) should be {}",
                &h[..8],
                if i >= 2 { "kept" } else { "evicted" }
            );
        }
    }

    /// A store at or under quota must not delete anything and must not need the
    /// eviction list at all.
    #[test]
    fn quota_under_limit_is_a_no_op() {
        let store = ChunkStore::new(soshal_test_util::tmp_root("cas_quota_noop"));
        store.put(&vec![7u8; 4096]);
        let before = store.total_bytes();
        assert!(before > 0);
        assert_eq!(store.enforce_cache_quota(u64::MAX).unwrap(), 0);
        assert_eq!(store.total_bytes(), before, "nothing may be deleted");
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

    #[test]
    fn test_empty_chunk_and_store_file_regular_check() {
        let root = soshal_test_util::tmp_root("cas_empty_and_dir");
        let store = ChunkStore::new(root.clone());

        // Empty chunk put & get
        let empty_data = b"";
        let empty_hash = blake3::hash(empty_data).to_hex().to_string();
        assert!(store.put(empty_data));
        assert!(store.contains(&empty_hash));
        assert_eq!(store.get(&empty_hash), Some(Vec::new()));
        assert!(store.get_mmap(&empty_hash).is_none());

        // store_file on directory must fail
        assert!(store.store_file(&root).is_err());
    }
}
