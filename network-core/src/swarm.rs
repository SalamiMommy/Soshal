//! Parallel swarm downloads into memory-mapped sparse files.
//!
//! A blob manifest is fetched (from a peer or relay) and its chunks are then
//! downloaded in parallel from the peer swarm: each worker pulls a chunk from
//! a peer, verifies its BLAKE3 hash, and writes it at its exact offset in a
//! mmap'd sparse file. Out-of-order writes land directly in the page cache at
//! the right position, so playback (or range serving via the local video
//! server) can start before the file is complete.
//!
//! Execution model mirrors `sync-core`'s engine: a dedicated thread owns a
//! multi-thread tokio runtime, independent of the Flutter isolate.

use crate::lan_transport::{fetch_chunk_range, LanChunkRequest};
use crate::quic::{fetch_quic_chunk, QuicChunkPool};
use soshal_media_core::chunking::ChunkManifest;
use std::collections::HashSet;
use std::fs::File;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Per-chunk fetch timeout against a single peer.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(10);

pub struct SwarmConfig {
    /// Verified chunk manifest describing the target blob.
    pub manifest: ChunkManifest,
    /// Destination file; created/sparsified at `total_size` up front.
    pub out_path: PathBuf,
    /// Direct peer addresses (already MAC-authenticated at discovery).
    pub peers: Vec<SocketAddr>,
    /// QUIC ports for each peer (if advertised via mDNS). Same length as `peers`.
    pub quic_ports: Vec<Option<u16>>,
    /// Identity-derived LAN key for the handshake.
    pub key: [u8; 32],
    pub my_pubkey: String,
    /// Parallel worker count (capped by the power scheduler for background use).
    pub max_parallel: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SwarmReport {
    pub verified_chunks: usize,
    pub bytes_downloaded: u64,
    pub failures: usize,
    pub failed_hashes: Vec<String>,
}

/// Spawns a swarm download on its own thread + tokio runtime.
pub fn spawn_swarm_download(cfg: SwarmConfig) -> std::thread::JoinHandle<SwarmReport> {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("soshal-swarm")
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::warn!("swarm: runtime init failed: {e}");
                return SwarmReport {
                    failures: cfg.manifest.chunks.len(),
                    ..SwarmReport::default()
                };
            }
        };
        rt.block_on(download(cfg))
    })
}

async fn download(cfg: SwarmConfig) -> SwarmReport {
    let total = cfg.manifest.total_size;
    let chunks = Arc::new(cfg.manifest.chunks.clone());
    if chunks.is_empty() || cfg.peers.is_empty() {
        return SwarmReport {
            failures: chunks.len(),
            failed_hashes: chunks.iter().map(|c| c.blake3.clone()).collect(),
            ..SwarmReport::default()
        };
    }

    let file = match File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&cfg.out_path)
    {
        Ok(f) => f,
        Err(e) => {
            log::warn!("swarm: open {}: {e}", cfg.out_path.display());
            return SwarmReport {
                failures: chunks.len(),
                failed_hashes: chunks.iter().map(|c| c.blake3.clone()).collect(),
                ..SwarmReport::default()
            };
        }
    };
    if let Err(e) = file.set_len(total) {
        log::warn!("swarm: sparse set_len: {e}");
        return SwarmReport {
            failures: chunks.len(),
            failed_hashes: chunks.iter().map(|c| c.blake3.clone()).collect(),
            ..SwarmReport::default()
        };
    }
    let mmap = match unsafe_mmap(&file) {
        Some(m) => m,
        None => {
            return SwarmReport {
                failures: chunks.len(),
                failed_hashes: chunks.iter().map(|c| c.blake3.clone()).collect(),
                ..SwarmReport::default()
            };
        }
    };
    let map = Arc::new(Mutex::new(mmap));
    let done = Arc::new(Mutex::new(HashSet::<usize>::new()));
    let next = Arc::new(AtomicUsize::new(0));
    let workers = cfg.max_parallel.clamp(1, cfg.peers.len() * 2).max(1);
    let key = cfg.key;
    let pubkey = cfg.my_pubkey.clone();
    let quic_ports = cfg.quic_ports.clone();

    // Shared QUIC client: one endpoint + one connection per peer, reused
    // across chunks (avoids a fresh socket + TLS handshake per chunk).
    let pool = if quic_ports.iter().any(|p| p.is_some()) {
        QuicChunkPool::new().ok().map(Arc::new)
    } else {
        None
    };

    let mut handles = Vec::new();
    for w in 0..workers {
        let chunks = chunks.clone();
        let peers = cfg.peers.clone();
        let quic_ports = quic_ports.clone();
        let pool = pool.clone();
        let map = map.clone();
        let done = done.clone();
        let next = next.clone();
        let pubkey = pubkey.clone();
        handles.push(tokio::spawn(async move {
            loop {
                // Slot index; emitted exactly once globally.
                let idx = next.fetch_add(1, Ordering::Relaxed);
                if idx >= chunks.len() {
                    break;
                }
                let chr = &chunks[idx];

                // Rotate peer on each attempt so failures don't collapse on
                // one peer; two attempts max per chunk. Prefer QUIC if advertised.
                let mut got = None;
                for attempt in 0..2 {
                    let peer_idx = (w + idx + attempt) % peers.len();
                    let peer = peers[peer_idx];
                    let quic_port = quic_ports.get(peer_idx).copied().flatten();
                    let offset = match usize::try_from(chr.offset) {
                        Ok(o) => o,
                        Err(_) => {
                            log::debug!("swarm: chunk {}: offset too large for usize", chr.blake3);
                            continue;
                        }
                    };
                    let req = LanChunkRequest {
                        hash: chr.blake3.clone(),
                        offset,
                        length: chr.len,
                        want_manifest: false,
                    };

                    // Prefer QUIC if the peer advertises a QUIC port; fall back to TCP.
                    // Both paths normalize to Result<Result<Vec<u8>, String>, Elapsed>.
                    let fetch_result = if let Some(qp) = quic_port {
                        let quic_addr = SocketAddr::new(peer.ip(), qp);
                        match &pool {
                            Some(pool) => {
                                let key = key;
                                let pubkey = pubkey.clone();
                                let req = req.clone();
                                tokio::time::timeout(
                                    CHUNK_TIMEOUT,
                                    pool.fetch_chunk(quic_addr, key, &pubkey, &req),
                                )
                                .await
                            }
                            None => {
                                let fetch = tokio::task::spawn_blocking({
                                    let key = key;
                                    let pubkey = pubkey.clone();
                                    let req = req.clone();
                                    move || fetch_quic_chunk(quic_addr, key, &pubkey, &req)
                                });
                                tokio::time::timeout(CHUNK_TIMEOUT, fetch).await.map(|r| {
                                    r.map_err(|je| format!("worker panic: {je}"))
                                        .and_then(|x| x)
                                })
                            }
                        }
                    } else {
                        let fetch = tokio::task::spawn_blocking({
                            let key = key;
                            let pubkey = pubkey.clone();
                            let req = req.clone();
                            move || fetch_chunk_range(peer, key, &pubkey, &req)
                        });
                        tokio::time::timeout(CHUNK_TIMEOUT, fetch).await.map(|r| {
                            r.map_err(|je| format!("worker panic: {je}"))
                                .and_then(|x| x)
                        })
                    };

                    match fetch_result {
                        Ok(Ok(data))
                            if blake3::Hash::from_hex(&chr.blake3)
                                .is_ok_and(|h| h == blake3::hash(&data)) =>
                        {
                            got = Some(data);
                            break;
                        }
                        Ok(Ok(_)) => {} // hash mismatch → next attempt
                        Ok(Err(e)) => log::debug!("swarm: peer {peer}: {e}"),
                        Err(_) => {} // timeout / panic → next attempt
                    }
                }

                match got {
                    Some(data) => {
                        // Hostile manifest guard: chunk range must fit the
                        // blob and the mmap. Checked arithmetic only — skip
                        // the chunk on overflow/OOB instead of panicking.
                        let end = match chr.offset.checked_add(data.len() as u64) {
                            Some(e) => e,
                            None => {
                                log::debug!("swarm: chunk {}: offset overflow", chr.blake3);
                                continue;
                            }
                        };
                        if end > total {
                            log::debug!(
                                "swarm: chunk {}: range {}-{} exceeds blob size {}",
                                chr.blake3,
                                chr.offset,
                                end,
                                total
                            );
                            continue;
                        }
                        let off = match usize::try_from(chr.offset) {
                            Ok(o) => o,
                            Err(_) => {
                                log::debug!(
                                    "swarm: chunk {}: offset too large for usize",
                                    chr.blake3
                                );
                                continue;
                            }
                        };
                        let copy_end = match off.checked_add(data.len()) {
                            Some(e) => e,
                            None => {
                                log::debug!("swarm: chunk {}: copy range overflow", chr.blake3);
                                continue;
                            }
                        };
                        {
                            let mut map = map.lock().unwrap();
                            if copy_end > map.len() {
                                log::debug!(
                                    "swarm: chunk {}: range {}-{} exceeds mmap len {}",
                                    chr.blake3,
                                    off,
                                    copy_end,
                                    map.len()
                                );
                                continue;
                            }
                            map[off..copy_end].copy_from_slice(&data);
                        }
                        done.lock().unwrap().insert(idx);
                    }
                    None => {
                        log::debug!("swarm: chunk {} failed on all peers", chr.blake3);
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let done_set = done.lock().unwrap();
    let verified = done_set.len();
    let mut report = SwarmReport {
        verified_chunks: verified,
        bytes_downloaded: done_set.iter().map(|&i| chunks[i].len as u64).sum(),
        ..SwarmReport::default()
    };
    report.failures = chunks.len() - verified;
    report.failed_hashes = chunks
        .iter()
        .enumerate()
        .filter(|(i, _)| !done_set.contains(i))
        .map(|(_, c)| c.blake3.clone())
        .collect();

    // Flush mmap'd pages to the file before dropping; drop un-maps after.
    if let Ok(map) = map.lock() {
        let _ = map.flush();
    }
    drop(map);

    log::debug!(
        "swarm: {} verified chunks, {} bytes ({} failures)",
        report.verified_chunks,
        report.bytes_downloaded,
        report.failures
    );
    report
}

/// memmap2's API is safe to *call* (no unsafe wrapper), but constructing a
/// `MmapMut` from a file is `unsafe` because:
///   1. Any out-of-process truncation of the underlying file can shrink the
///      mapped region, making the Rust `&mut [u8]` slice dangle — UB.
///   2. Any other mapping of the same file produces aliased mutable references
///      if both sides write — also UB.
///
/// # Safety
/// This call is sound because:
/// - The file is created exclusively by this process and held open for the
///   full lifetime of the mmap (no POSIX `close` until `drop(map)`).  No
///   other process can truncate or unlink it while we hold the fd.
/// - `MmapMut` is wrapped in `Arc<Mutex<…>>` so no two tasks ever hold
///   overlapping `&mut [u8]` references simultaneously.
#[allow(unsafe_code)]
fn unsafe_mmap(file: &File) -> Option<memmap2::MmapMut> {
    // SAFETY: see module-level comment above.
    unsafe { memmap2::MmapMut::map_mut(file).ok() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan_transport::start_lan_server_with_store;
    use crate::lan_transport::tests::start_hostile_lan_server;
    use soshal_media_core::cas::ChunkStore;
    use soshal_media_core::chunking::{ChunkManifest, ChunkRef};

    fn empty_manifest() -> ChunkManifest {
        ChunkManifest {
            blob_hash: "ab".repeat(32),
            total_size: 0,
            chunks: vec![],
        }
    }

    fn one_chunk_manifest() -> ChunkManifest {
        ChunkManifest {
            blob_hash: "cd".repeat(32),
            total_size: 4,
            chunks: vec![ChunkRef {
                blake3: "ef".repeat(32),
                offset: 0,
                len: 4,
            }],
        }
    }

    #[tokio::test]
    async fn swarm_early_return_on_empty_chunks() {
        let root = soshal_test_util::tmp_root("swarm_empty");
        let out = root.join("out.bin");
        let cfg = SwarmConfig {
            manifest: empty_manifest(),
            out_path: out.clone(),
            peers: vec![],
            quic_ports: vec![],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 2,
        };
        let report = download(cfg).await;
        assert_eq!(report.failures, 0);
        assert!(report.failed_hashes.is_empty());
        assert!(!out.exists());

        // Peers present but zero chunks hits the same early return.
        let cfg = SwarmConfig {
            manifest: empty_manifest(),
            out_path: out,
            peers: vec!["127.0.0.1:1".parse().unwrap()],
            quic_ports: vec![None],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 2,
        };
        let report = download(cfg).await;
        assert_eq!(report.failures, 0);
    }

    #[tokio::test]
    async fn swarm_early_return_on_unwritable_out_path() {
        let root = soshal_test_util::tmp_root("swarm_unwritable");
        let cfg = SwarmConfig {
            manifest: one_chunk_manifest(),
            out_path: root.join("missing-dir").join("out.bin"),
            peers: vec!["127.0.0.1:1".parse().unwrap()],
            quic_ports: vec![None],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 2,
        };
        let report = download(cfg).await;
        assert_eq!(report.failures, 1);
        assert_eq!(report.failed_hashes, vec!["ef".repeat(32)]);
    }

    #[test]
    fn swarm_spawn_thread_reports_fast_on_empty() {
        let root = soshal_test_util::tmp_root("swarm_spawn");
        let handle = spawn_swarm_download(SwarmConfig {
            manifest: empty_manifest(),
            out_path: root.join("out.bin"),
            peers: vec![],
            quic_ports: vec![],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 2,
        });
        let report = handle.join().unwrap();
        assert_eq!(report.failures, 0);
        assert!(report.failed_hashes.is_empty());
    }

    #[test]
    fn mmap_sparse_rescue_on_failed_path() {
        let root = soshal_test_util::tmp_root("swarm_test");
        std::fs::create_dir_all(&root).unwrap();
        let store = ChunkStore::new(root.join("chunks"));
        let mut data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        data.extend_from_slice(&[9u8; 256 * 1024]);
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        // QUIC-only seeder: swarm must prefer the advertised QUIC port and
        // never dial TCP (no LAN server is started).
        let quic = crate::quic::start_quic_stream_server_with_store([7u8; 32], root.join("chunks"))
            .unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], quic.port));
        let peers = vec![addr];
        let quic_ports = vec![Some(quic.port)];

        let out = root.join("out.bin");
        let handle = spawn_swarm_download(SwarmConfig {
            manifest: m.clone(),
            out_path: out.clone(),
            peers: peers.clone(),
            quic_ports,
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 4,
        });
        let report = handle.join().unwrap();
        assert_eq!(report.failed_hashes.len(), 0, "all chunks verified");
        assert_eq!(report.verified_chunks, m.chunks.len());
        assert_eq!(report.bytes_downloaded, m.total_size);
        quic.stop();

        let on_disk = std::fs::read(&out).unwrap();
        assert_eq!(on_disk.len(), m.total_size as usize);
        assert_eq!(on_disk, data, "reassembled blob matches source");
    }

    #[test]
    fn empty_peers_fails_cleanly() {
        let root = soshal_test_util::tmp_root("swarm_test");
        std::fs::create_dir_all(&root).unwrap();
        let store = ChunkStore::new(root.join("chunks"));
        let m = store
            .store_reader(std::io::Cursor::new(&[1u8; 300 * 1024]))
            .unwrap();
        let handle = spawn_swarm_download(SwarmConfig {
            manifest: m.clone(),
            out_path: root.join("out2.bin"),
            peers: vec![],
            quic_ports: vec![],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 2,
        });
        let report = handle.join().unwrap();
        assert_eq!(report.failures, m.chunks.len());
    }

    #[test]
    fn swarm_rotates_off_corrupt_peer_and_recovers() {
        let root = soshal_test_util::tmp_root("swarm_test");
        let store = ChunkStore::new(root.join("chunks"));
        let data: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        // Peer 0 hostile (garbage bytes, never verifies); peer 1 honest.
        // Attempt 1 rotates from 0 to 1 and the download recovers.
        let mut hostile = start_hostile_lan_server();
        let mut server = start_lan_server_with_store([7u8; 32], root.join("chunks")).unwrap();
        let peers = vec![
            SocketAddr::from(([127, 0, 0, 1], hostile.port)),
            SocketAddr::from(([127, 0, 0, 1], server.port)),
        ];
        let out = root.join("out.bin");
        let handle = spawn_swarm_download(SwarmConfig {
            manifest: m.clone(),
            out_path: out.clone(),
            peers,
            quic_ports: vec![None, None],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 1,
        });
        let report = handle.join().unwrap();
        assert_eq!(report.verified_chunks, m.chunks.len());
        assert_eq!(report.failures, 0);
        assert!(report.failed_hashes.is_empty());
        assert_eq!(std::fs::read(&out).unwrap(), data);
        server.stop();
        hostile.stop();
    }

    #[test]
    fn swarm_hash_mismatch_fails_and_hostile_ranges_are_guarded() {
        let root = soshal_test_util::tmp_root("swarm_test");
        let store = ChunkStore::new(root.join("chunks"));
        let data: Vec<u8> = (0..100 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        // Single hostile peer: both attempts mismatch → failure report.
        let mut hostile = start_hostile_lan_server();
        let addr = SocketAddr::from(([127, 0, 0, 1], hostile.port));
        let handle = spawn_swarm_download(SwarmConfig {
            manifest: m.clone(),
            out_path: root.join("out1.bin"),
            peers: vec![addr],
            quic_ports: vec![None],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 1,
        });
        let report = handle.join().unwrap();
        assert_eq!(report.verified_chunks, 0);
        assert_eq!(report.failures, m.chunks.len());
        assert_eq!(
            report.failed_hashes,
            m.chunks
                .iter()
                .map(|c| c.blake3.clone())
                .collect::<Vec<_>>()
        );
        hostile.stop();

        // Honest peer, hostile manifest: verified chunk ends past total_size
        // → the end > total guard skips instead of panicking.
        let mut server = start_lan_server_with_store([7u8; 32], root.join("chunks")).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));
        let mut hostile_manifest = m.clone();
        hostile_manifest.total_size = 1;
        let handle = spawn_swarm_download(SwarmConfig {
            manifest: hostile_manifest,
            out_path: root.join("out2.bin"),
            peers: vec![addr],
            quic_ports: vec![None],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 1,
        });
        let report = handle.join().unwrap();
        assert_eq!(report.verified_chunks, 0);
        assert_eq!(report.failures, m.chunks.len());
        assert_eq!(
            report.failed_hashes,
            m.chunks
                .iter()
                .map(|c| c.blake3.clone())
                .collect::<Vec<_>>()
        );
        server.stop();
    }
}
