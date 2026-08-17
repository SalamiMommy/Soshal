//! Coverage tests for CAS fns with no test refs in the inline module:
//! `put_trusted`, `write_chunk_to`, `default_root`.

use std::fs;
use std::io::Cursor;
use std::sync::Mutex;

use soshal_media_core::cas::ChunkStore;
use soshal_media_core::chunking::ChunkRef;

/// Serializes tests that mutate the `SOSHAL_CHUNK_CACHE` env var.
static ENV_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// put_trusted
// ---------------------------------------------------------------------------

#[test]
fn put_trusted_writes_under_precomputed_hash() {
    let store = ChunkStore::new(soshal_test_util::tmp_root("cas_cov"));
    let data: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
    let hash = blake3::hash(&data).to_hex().to_string();

    assert!(store.put_trusted(&hash, &data));
    assert!(store.contains(&hash));
    assert_eq!(store.get(&hash).unwrap(), data);
    // Existing slot is never rewritten (idempotent dedup).
    assert!(!store.put_trusted(&hash, &data));
}

#[test]
fn put_trusted_skips_rehashing() {
    let store = ChunkStore::new(soshal_test_util::tmp_root("cas_cov"));
    let data = vec![9u8; 1024];
    let other = blake3::hash(b"different bytes").to_hex().to_string();

    // Trusted semantics: the caller asserted the hash, so the store writes
    // the bytes verbatim under the given key without verifying them. The
    // mismatch is caught later on read: `get` re-verifies and refuses.
    assert!(store.put_trusted(&other, &data));
    assert!(store.contains(&other));
    assert!(store.get(&other).is_none());
}

#[test]
fn put_trusted_handles_tmp_file_collisions() {
    let store = ChunkStore::new(soshal_test_util::tmp_root("cas_cov"));
    let data = vec![4u8; 512];
    let hash = blake3::hash(&data).to_hex().to_string();
    let path = store.chunk_path(&hash);
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    // Pre-seed stale temp files so both collision branches of the retry loop
    // run: `.tmp`, then `.{n}` suffixed.
    fs::write(path.with_extension("tmp"), b"stale").unwrap();
    fs::write(format!("{}.1", path.to_string_lossy()), b"stale").unwrap();

    assert!(store.put_trusted(&hash, &data));
    assert_eq!(store.get(&hash).unwrap(), data);
}

#[test]
fn put_trusted_fails_when_parent_dir_not_creatable() {
    let root = soshal_test_util::tmp_root("cas_cov");
    let blocker = root.join("blocker");
    fs::write(&blocker, b"x").unwrap();

    // Root is a file, so chunk-path parents can never be created.
    let store = ChunkStore::new(blocker);
    let hash = blake3::hash(b"data").to_hex().to_string();
    assert!(!store.put_trusted(&hash, b"data"));
}

// ---------------------------------------------------------------------------
// write_chunk_to
// ---------------------------------------------------------------------------

#[test]
fn write_chunk_to_streams_exact_chunk_bytes() {
    let store = ChunkStore::new(soshal_test_util::tmp_root("cas_cov"));
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 253) as u8).collect();
    let m = store.store_reader(Cursor::new(&data)).unwrap();

    for chr in &m.chunks {
        let mut out = Vec::new();
        store.write_chunk_to(chr, &mut out).unwrap();
        assert_eq!(out, store.get(&chr.blake3).unwrap());
        assert_eq!(out.len(), chr.len);
        assert_eq!(out, data[chr.offset as usize..][..chr.len]);
    }
}

#[test]
fn write_chunk_to_missing_chunk_errors() {
    let store = ChunkStore::new(soshal_test_util::tmp_root("cas_cov"));
    let chr = ChunkRef {
        blake3: "ab".repeat(32),
        offset: 0,
        len: 16,
    };
    let mut out = Vec::new();
    let err = store.write_chunk_to(&chr, &mut out).unwrap_err();
    assert!(err.contains("missing"));
    assert!(out.is_empty());
}

// ---------------------------------------------------------------------------
// default_root
// ---------------------------------------------------------------------------

#[test]
fn default_root_respects_env_override_and_fallback() {
    let _guard = ENV_LOCK.lock().unwrap();
    let custom = soshal_test_util::tmp_root("cas_cov_root");

    std::env::set_var("SOSHAL_CHUNK_CACHE", &custom);
    assert_eq!(ChunkStore::default_root(), custom);

    std::env::remove_var("SOSHAL_CHUNK_CACHE");
    assert_eq!(
        ChunkStore::default_root(),
        std::env::temp_dir().join("soshal_chunks")
    );
}
