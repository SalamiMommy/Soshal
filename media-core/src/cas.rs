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
use std::sync::{Arc, RwLock};

use crate::chunking::{chunk_reader_with_data, ChunkManifest, ChunkRef};

/// Chunk files are plaintext cache content. At-rest encryption of the app DB
/// is untouched; the CAS lives in cache space and is evictable.
#[derive(Clone)]
pub struct ChunkStore {
    root: PathBuf,
    /// Lazy chunk-hash → owning manifest cache. Resolves peer "do you have
    /// chunk X" requests without re-reading every manifests/*.json.
    index: Arc<RwLock<Option<Arc<ChunkIndex>>>>,
}

/// `chunk_hash -> blob_hash` map plus the chunk's offset inside that blob.
/// Built once from the manifests directory, kept incrementally in sync on
/// `save_manifest`, and dropped wholesale on `clear`.
#[derive(Default)]
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

impl ChunkStore {
    /// `root` should point at the cache dir (e.g. `<cache>/chunks`).
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            index: Arc::new(RwLock::new(None)),
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
                *slot = Some(built.clone());
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
        let (a, b) = hash.split_at(2);
        self.root.join(a).join(format!("{b}.chunk"))
    }

    pub fn contains(&self, hash: &str) -> bool {
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
        if hash.len() != 64 {
            return None;
        }
        let path = self.chunk_path(hash);
        let file = fs::File::open(&path).ok()?;
        // Safety: read-only mapping of a file we opened read-only; the Mmap is
        // the sole handle to the region and unmap happens on drop.
        #[allow(unsafe_code)]
        let map = unsafe { memmap2::Mmap::map(&file).ok()? };
        if blake3::hash(map.as_ref()).to_hex().as_str() != hash {
            return None;
        }
        Some(map)
    }

    /// Stores a chunk. Returns `true` if newly written, `false` if it already
    /// existed (or the hash didn't match — slot is never poisoned).
    /// Stores a chunk. Returns `true` if newly written, `false` if it already
    /// existed (or the hash didn't match — slot is never poisoned).
    pub fn put(&self, data: &[u8]) -> bool {
        let hash = blake3::hash(data).to_hex().to_string();
        self.put_trusted(&hash, data)
    }

    /// Stores a chunk under a trusted pre-calculated hash without re-hashing data.
    pub fn put_trusted(&self, hash: &str, data: &[u8]) -> bool {
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
        if let Ok(mut slot) = self.index.write() {
            *slot = None;
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
    pub fn store_reader<R: Read>(&self, reader: R) -> Result<ChunkManifest, String> {
        let (manifest, chunks) = chunk_reader_with_data(reader)?;
        for (c, data) in chunks {
            self.put_trusted(&c.blake3, &data);
        }
        Ok(manifest)
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

    /// Removes all chunks (cache clear).
    pub fn clear(&self) {
        let _ = fs::remove_dir_all(&self.root);
        if let Ok(mut slot) = self.index.write() {
            *slot = None;
        }
    }

    /// Default on-disk location used by peer-serving and bridge call sites.
    pub fn default_root() -> PathBuf {
        std::env::var_os("SOSHAL_CHUNK_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("soshal_chunks"))
    }

    /// Manifest persistence lives next to the chunk files so peer-serving
    /// (which has no DB access) can answer blob-range requests from disk alone.
    pub fn manifest_path(&self, blob_hash: &str) -> PathBuf {
        self.root
            .join("manifests")
            .join(format!("{blob_hash}.json"))
    }

    pub fn save_manifest(&self, manifest: &ChunkManifest) -> Result<(), String> {
        let path = self.manifest_path(&manifest.blob_hash);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("manifest dir: {e}"))?;
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
            if let Some(idx) = Arc::get_mut(idx) {
                idx.ingest(manifest);
            }
        }
        Ok(())
    }

    pub fn load_manifest(&self, blob_hash: &str) -> Option<ChunkManifest> {
        if blob_hash.len() != 64 {
            return None;
        }
        let raw = fs::read_to_string(self.manifest_path(blob_hash)).ok()?;
        let m: ChunkManifest = serde_json::from_str(&raw).ok()?;
        if m.blob_hash == blob_hash && m.is_valid() {
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
            let mmap = self.get_mmap(&c.blake3)?;
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
        let mut b = shared.clone();
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
}
