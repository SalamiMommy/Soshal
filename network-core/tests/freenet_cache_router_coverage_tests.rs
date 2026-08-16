//! Coverage tests for the CAS + fallback file-cache branches of
//! `freenet_cache_router::process_freenet_cache_command`, which the inline
//! mod and `freenet_cache_router_tests.rs` never seed.
//!
//! The CAS branch reads `ChunkStore::default_root()` (env `SOSHAL_CHUNK_CACHE`
//! or `<temp>/soshal_chunks`) and the fallback reads
//! `<temp>/soshal_media_cache/<hash>.bin`. Both are process-global paths, so
//! every test here holds `soshal_test_util::test_lock()` and cleans up after
//! itself.

use base64::Engine;
use serde_json::json;
use soshal_media_core::cas::ChunkStore;
use soshal_media_core::chunking::{ChunkManifest, ChunkRef};
use soshal_network_core::freenet_cache_router::{
    merge_freenet_cache_responses, process_freenet_cache_command, CacheQueryResult,
};
use soshal_network_core::p2p_frame::FreenetP2PCommand;

/// Runs `f` with `SOSHAL_CHUNK_CACHE` pointed at a fresh temp root, then
/// restores the previous value. Serialized via the shared test lock because
/// the env var is process-global.
fn with_chunk_cache_root(f: impl FnOnce(&std::path::Path)) {
    let _lock = soshal_test_util::test_lock();
    let root = soshal_test_util::tmp_root("freenet_cas");
    let old = std::env::var_os("SOSHAL_CHUNK_CACHE");
    std::env::set_var("SOSHAL_CHUNK_CACHE", &root);
    f(&root);
    match old {
        Some(v) => std::env::set_var("SOSHAL_CHUNK_CACHE", v),
        None => std::env::remove_var("SOSHAL_CHUNK_CACHE"),
    }
}

fn media_blob(hash: &str, offset: usize, length: usize) -> FreenetP2PCommand {
    FreenetP2PCommand::GetMediaBlob {
        hash: hash.to_string(),
        chunk_offset: offset,
        chunk_length: length,
    }
}

fn decode_b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .expect("valid base64")
}

fn blob_data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

#[test]
fn cas_seeded_serves_blob_range() {
    with_chunk_cache_root(|root| {
        let store = ChunkStore::new(root.to_path_buf());
        let data = blob_data(300 * 1024);
        let manifest = store
            .store_reader(std::io::Cursor::new(&data))
            .expect("stored");
        store.save_manifest(&manifest).unwrap();

        let cmd = media_blob(&manifest.blob_hash, 4096, 70_000);
        let result = process_freenet_cache_command(&cmd, &[], "self").expect("CAS hit");
        match result {
            FreenetP2PCommand::MediaBlobResponse {
                hash,
                offset,
                total_size,
                data_b64,
            } => {
                assert_eq!(hash, manifest.blob_hash);
                assert_eq!(offset, 4096);
                assert_eq!(total_size, data.len());
                assert_eq!(decode_b64(&data_b64), &data[4096..4096 + 70_000]);
            }
            other => panic!("expected MediaBlobResponse, got {other:?}"),
        }
    });
}

#[test]
fn cas_manifest_with_missing_chunk_not_served() {
    with_chunk_cache_root(|root| {
        let store = ChunkStore::new(root.to_path_buf());
        let data = blob_data(1024 * 1024);
        // Valid manifest (64-hex chunk key, contiguous coverage) whose chunk
        // was never written to disk — `blob_slice` must refuse to serve it.
        let manifest = ChunkManifest {
            blob_hash: blake3::hash(&data).to_hex().to_string(),
            total_size: data.len() as u64,
            chunks: vec![ChunkRef {
                blake3: "ab".repeat(32),
                offset: 0,
                len: data.len(),
            }],
        };
        store.save_manifest(&manifest).unwrap();

        let cmd = media_blob(&manifest.blob_hash, 0, 1024);
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    });
}

#[test]
fn cas_empty_manifest_not_served() {
    with_chunk_cache_root(|root| {
        let store = ChunkStore::new(root.to_path_buf());
        let (manifest, _) =
            soshal_media_core::chunking::chunk_reader_with_data(std::io::Cursor::new(&[])).unwrap();
        store.save_manifest(&manifest).unwrap();

        let cmd = media_blob(&manifest.blob_hash, 0, 1024);
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    });
}

#[test]
fn cas_manifest_out_of_range_returns_none() {
    with_chunk_cache_root(|root| {
        let store = ChunkStore::new(root.to_path_buf());
        let data = blob_data(64 * 1024);
        let (manifest, _) =
            soshal_media_core::chunking::chunk_reader_with_data(std::io::Cursor::new(&data))
                .unwrap();
        store.save_manifest(&manifest).unwrap();

        // Offset at EOF with a non-zero length exceeds the blob.
        let cmd = media_blob(&manifest.blob_hash, data.len(), 1);
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    });
}

#[test]
fn cas_bypassed_when_chunk_length_exceeds_max() {
    with_chunk_cache_root(|root| {
        let store = ChunkStore::new(root.to_path_buf());
        let data = blob_data(64 * 1024);
        let (manifest, _) =
            soshal_media_core::chunking::chunk_reader_with_data(std::io::Cursor::new(&data))
                .unwrap();
        store.save_manifest(&manifest).unwrap();

        // 1 MiB + 1 skips the CAS guard entirely; no fallback file exists.
        let cmd = media_blob(&manifest.blob_hash, 0, 1024 * 1024 + 1);
        assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());
    });
}

#[test]
fn fallback_file_cache_serves_whole_file() {
    let _lock = soshal_test_util::test_lock();
    let data = blob_data(512);
    let hash = blake3::hash(&data).to_hex().to_string();
    let dir = std::env::temp_dir().join("soshal_media_cache");
    let path = dir.join(format!("{hash}.bin"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, &data).unwrap();

    // First hit: head of the file.
    let cmd = media_blob(&hash, 0, 100);
    let result = process_freenet_cache_command(&cmd, &[], "self").expect("file-cache hit");
    match result {
        FreenetP2PCommand::MediaBlobResponse {
            hash: h,
            offset,
            total_size,
            data_b64,
        } => {
            assert_eq!(h, hash);
            assert_eq!(offset, 0);
            assert_eq!(total_size, data.len());
            assert_eq!(decode_b64(&data_b64), &data[..100]);
        }
        other => panic!("expected MediaBlobResponse, got {other:?}"),
    }

    // Second hit: a different range of the same cached file.
    let cmd = media_blob(&hash, 300, 100);
    let result = process_freenet_cache_command(&cmd, &[], "self").expect("second file-cache hit");
    match result {
        FreenetP2PCommand::MediaBlobResponse {
            offset, data_b64, ..
        } => {
            assert_eq!(offset, 300);
            assert_eq!(decode_b64(&data_b64), &data[300..400]);
        }
        other => panic!("expected MediaBlobResponse, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn fallback_file_cache_offset_past_end_returns_none() {
    let _lock = soshal_test_util::test_lock();
    let data = blob_data(100);
    let hash = blake3::hash(&data).to_hex().to_string();
    let dir = std::env::temp_dir().join("soshal_media_cache");
    let path = dir.join(format!("{hash}.bin"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, &data).unwrap();

    // Offset at EOF: the `offset < len` guard fails and the router yields None.
    let cmd = media_blob(&hash, 100, 1);
    assert!(process_freenet_cache_command(&cmd, &[], "self").is_none());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn get_post_cache_filters_post_without_pubkey() {
    let cmd = FreenetP2PCommand::GetPostCache {
        authors: vec!["npub1".to_string()],
        since: 0,
        limit: 10,
        wot_distance: 1,
        allow_2hop: false,
    };
    let local_posts = vec![
        json!({"id": "a", "pubkey": "npub1", "created_at": 1}),
        json!({"id": "b", "created_at": 2}), // no pubkey -> filtered out
        json!({"id": "c", "pubkey": "npub2", "created_at": 3}),
    ];
    let result = process_freenet_cache_command(&cmd, &local_posts, "self").unwrap();
    match result {
        FreenetP2PCommand::PostCacheResponse { posts, .. } => {
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0]["id"], "a");
        }
        other => panic!("expected PostCacheResponse, got {other:?}"),
    }
}

#[test]
fn get_freenet_contract_missing_signature_and_non_string_key() {
    // Match without a `sig` field: signature defaults to empty string.
    let cmd = FreenetP2PCommand::GetFreenetContract {
        contract_key: "k1".to_string(),
        requester_pubkey: "r".to_string(),
        max_hops: 3,
    };
    let local_posts = vec![json!({"freenet_key": "k1"})];
    match process_freenet_cache_command(&cmd, &local_posts, "self").unwrap() {
        FreenetP2PCommand::FreenetContractResponse { signature, .. } => {
            assert_eq!(signature, "");
        }
        other => panic!("expected FreenetContractResponse, got {other:?}"),
    }

    // Non-string freenet_key never matches.
    let cmd = FreenetP2PCommand::GetFreenetContract {
        contract_key: "k2".to_string(),
        requester_pubkey: "r".to_string(),
        max_hops: 3,
    };
    let local_posts = vec![json!({"freenet_key": ["k2"]})];
    assert!(process_freenet_cache_command(&cmd, &local_posts, "self").is_none());
}

#[test]
fn merge_skips_posts_without_id() {
    let responses = vec![CacheQueryResult {
        posts: vec![
            json!({"created_at": 1000}),
            json!({"id": "post1", "created_at": 2000}),
        ],
        source_wot_distance: 1,
        peer_pubkey: "peer1".to_string(),
    }];
    let merged = merge_freenet_cache_responses(responses);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0]["id"], "post1");
}

#[test]
fn merge_keeps_existing_wot_tag() {
    let responses = vec![CacheQueryResult {
        posts: vec![json!({"id": "post1", "wot_source_distance": 7, "created_at": 1000})],
        source_wot_distance: 2,
        peer_pubkey: "peer1".to_string(),
    }];
    let merged = merge_freenet_cache_responses(responses);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0]["wot_source_distance"], 7);
}
