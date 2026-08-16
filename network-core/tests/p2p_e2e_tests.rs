//! End-to-end LAN P2P tests: chunk store → HMAC LAN server → swarm download.
//!
//! Exercises the same path the Flutter bridge wires up, at the core level:
//! a chunk store seeds a blob over loopback and a swarm downloader pulls it
//! into a sparse mmap'd file, verifying every chunk's BLAKE3 before
//! considering it landed.

use soshal_media_core::cas::ChunkStore;
use soshal_media_core::chunking::{ChunkManifest, ChunkRef};
use soshal_network_core::lan_transport::start_lan_server_with_store;
use soshal_network_core::swarm::{spawn_swarm_download, SwarmConfig};
use std::net::SocketAddr;

#[test]
fn swarm_download_roundtrip_over_loopback() {
    let _g = soshal_test_util::test_lock();
    let root = soshal_test_util::tmp_root("store");
    let store = ChunkStore::new(root.clone());
    let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let manifest = store.store_reader(std::io::Cursor::new(&data)).unwrap();
    store.save_manifest(&manifest).unwrap();

    let mut server = start_lan_server_with_store([7u8; 32], root).unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

    let out_dir = soshal_test_util::tmp_root("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = out_dir.join("blob.bin");
    let handle = spawn_swarm_download(SwarmConfig {
        manifest: manifest.clone(),
        out_path: out.clone(),
        peers: vec![addr],
        quic_ports: vec![None],
        key: [7u8; 32],
        my_pubkey: "ab".repeat(32),
        max_parallel: 4,
    });
    let report = handle.join().unwrap();

    assert_eq!(report.verified_chunks, manifest.chunks.len());
    assert_eq!(report.failures, 0);
    assert_eq!(report.bytes_downloaded as usize, data.len());

    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), data.len());
    assert_eq!(
        soshal_crypto_core::hash::blake3_hash_hex(&got),
        manifest.blob_hash
    );
    server.stop();
}

#[test]
fn swarm_download_fails_cleanly_without_server() {
    let _g = soshal_test_util::test_lock();
    let manifest = ChunkManifest {
        blob_hash: "ab".repeat(32),
        total_size: 1024,
        chunks: vec![ChunkRef {
            blake3: "cd".repeat(32),
            offset: 0,
            len: 1024,
        }],
    };

    let out_dir = soshal_test_util::tmp_root("nope");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = out_dir.join("blob.bin");
    let handle = spawn_swarm_download(SwarmConfig {
        manifest,
        out_path: out,
        peers: vec![SocketAddr::from(([127, 0, 0, 1], 1))],
        quic_ports: vec![None],
        key: [7u8; 32],
        my_pubkey: "ab".repeat(32),
        max_parallel: 2,
    });
    let report = handle.join().unwrap();
    assert_eq!(report.verified_chunks, 0);
    assert!(report.failures > 0);
    assert!(!report.failed_hashes.is_empty());
}

#[test]
fn swarm_download_rejects_public_peer() {
    let _g = soshal_test_util::test_lock();
    let manifest = ChunkManifest {
        blob_hash: "ab".repeat(32),
        total_size: 1024,
        chunks: vec![ChunkRef {
            blake3: "cd".repeat(32),
            offset: 0,
            len: 1024,
        }],
    };
    let out_dir = soshal_test_util::tmp_root("pub");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = out_dir.join("blob.bin");
    let handle = spawn_swarm_download(SwarmConfig {
        manifest,
        out_path: out,
        peers: vec![SocketAddr::from(([8, 8, 8, 8], 9999))],
        quic_ports: vec![None],
        key: [7u8; 32],
        my_pubkey: "ab".repeat(32),
        max_parallel: 2,
    });
    let report = handle.join().unwrap();
    assert!(report.failures > 0);
}
