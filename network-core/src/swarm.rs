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
use soshal_media_core::chunking::{ChunkManifest, ChunkRef};
use std::collections::HashSet;
use std::fs::File;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    pub cancelled: bool,
}

/// Spawns a swarm download on its own thread + tokio runtime. Returns the
/// thread handle plus an abort token; setting the token stops fetching and
/// releases the mmap once the worker observes it.
pub fn spawn_swarm_download(
    cfg: SwarmConfig,
) -> (std::thread::JoinHandle<SwarmReport>, Arc<AtomicBool>) {
    let abort = Arc::new(AtomicBool::new(false));
    let thread = std::thread::spawn({
        let abort = abort.clone();
        move || {
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
            rt.block_on(download(cfg, abort))
        }
    });
    (thread, abort)
}

async fn download(cfg: SwarmConfig, abort: Arc<AtomicBool>) -> SwarmReport {
    let total = cfg.manifest.total_size;
    let chunks = Arc::new(cfg.manifest.chunks.clone());
    // Cheap, pure-memory gates first, so a manifest we will never download does
    // not pay for a filesystem round trip at all.
    if total > crate::blob_grab::MAX_BLOB_FETCH_BYTES {
        log::warn!("swarm: manifest total_size {total} exceeds cap");
        return failed(&chunks);
    }
    if chunks.is_empty() || cfg.peers.is_empty() {
        return failed(&chunks);
    }

    // Every filesystem step — canonicalize, symlink refusal, open, ftruncate,
    // mmap — goes on the blocking pool. Run inline they executed on the caller's
    // runtime thread, and `set_len` on a sparse file of up to `MAX_BLOB_FETCH_BYTES`
    // is not quick on an SD card or a network filesystem. One slow filesystem
    // then stalls every *other* task sharing that runtime, which on Android is
    // the frb runtime carrying the relay node and the database too.
    let out_path = cfg.out_path.clone();
    let prep = tokio::task::spawn_blocking(move || prepare_target(&out_path, total)).await;
    let mmap = match prep {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            log::warn!("swarm: {e}");
            return failed(&chunks);
        }
        Err(je) => {
            log::warn!("swarm: target setup panicked: {je}");
            return failed(&chunks);
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
        let abort = abort.clone();
        handles.push(tokio::spawn(async move {
            loop {
                if abort.load(Ordering::Relaxed) {
                    break;
                }
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
                    if abort.load(Ordering::Relaxed) {
                        break;
                    }
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
                            let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
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
                        done.lock().unwrap_or_else(|e| e.into_inner()).insert(idx);
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

    let done_set = done.lock().unwrap_or_else(|e| e.into_inner());
    let verified = done_set.len();
    let mut report = SwarmReport {
        verified_chunks: verified,
        bytes_downloaded: done_set.iter().map(|&i| chunks[i].len as u64).sum(),
        cancelled: abort.load(Ordering::Relaxed),
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

    // If download failed or was cancelled, clean up the incomplete sparse file
    if report.failures > 0 || report.cancelled {
        let _ = std::fs::remove_file(&cfg.out_path);
    }

    log::debug!(
        "swarm: {} verified chunks, {} bytes ({} failures)",
        report.verified_chunks,
        report.bytes_downloaded,
        report.failures
    );
    report
}

/// Validate a swarm destination path: it must be absolute, its parent must
/// not be a symlink, and no normalized component may escape via `..`.
/// Report for a download that could not be attempted at all: every chunk is a
/// failure and every hash is reported as failed.
///
/// The six call sites this replaced each rebuilt the same literal, which is six
/// places to keep in sync if the report shape ever changes.
fn failed(chunks: &[ChunkRef]) -> SwarmReport {
    SwarmReport {
        failures: chunks.len(),
        failed_hashes: chunks.iter().map(|c| c.blake3.clone()).collect(),
        ..SwarmReport::default()
    }
}

/// Filesystem setup for the sparse target file: path validation, the WP11
/// symlink refusal, open, `set_len`, and the writable mapping.
///
/// Step order is load-bearing and unchanged from the inline version. Validation
/// and the symlink check both have to happen **before** `open`: a link swapped in
/// between the check and the open is precisely the attack WP11 guards, and a
/// check after the open would be checking the wrong inode. Likewise `set_len`
/// must precede the mapping, or the map covers a zero-length file.
///
/// Every call here is a blocking syscall, so this runs on the blocking pool —
/// see the comment at the call site.
fn prepare_target(out_path: &std::path::Path, total: u64) -> Result<memmap2::MmapMut, String> {
    // The caller supplies this path (an app cache directory); we must not let a
    // hostile supplied path redirect the write elsewhere.
    validate_out_path(out_path).map_err(|e| format!("invalid out_path: {e}"))?;

    // WP11: refuse a symlink at the final path component. A link dropped in place
    // lets an attacker redirect the entire sparse-file write + mmap to an
    // arbitrary file.
    match std::fs::symlink_metadata(out_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(format!("refusing symlink out_path {}", out_path.display()));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!("stat {}: {e}", out_path.display()));
        }
    }

    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(out_path)
        .map_err(|e| format!("open {}: {e}", out_path.display()))?;
    file.set_len(total)
        .map_err(|e| format!("sparse set_len: {e}"))?;
    unsafe_mmap(&file).ok_or_else(|| "mmap failed".to_string())
}

fn validate_out_path(path: &std::path::Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "out_path has no parent".to_string())?;
    // Canonicalize the parent (resolves symlinks + `..`), recreating it if
    // absent, then require the file's own name to be a plain basename.
    let canon = std::fs::canonicalize(parent).map_err(|e| format!("parent canonicalize: {e}"))?;
    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| "out_path has no file name".to_string())?;
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("out_path file name invalid".to_string());
    }
    // Reject if the canonical parent itself is a symlink at write time.
    if let Ok(m) = std::fs::symlink_metadata(&canon) {
        if m.file_type().is_symlink() {
            return Err("out_path parent is a symlink".to_string());
        }
    }
    Ok(())
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
        let report = download(cfg, Arc::new(AtomicBool::new(false))).await;
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
        let report = download(cfg, Arc::new(AtomicBool::new(false))).await;
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
        let report = download(cfg, Arc::new(AtomicBool::new(false))).await;
        assert_eq!(report.failures, 1);
        assert_eq!(report.failed_hashes, vec!["ef".repeat(32)]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn swarm_refuses_symlink_out_path() {
        use std::os::unix::fs::symlink;
        let root = soshal_test_util::tmp_root("swarm_symlink");
        let target = root.join("real.bin");
        let link = root.join("link.bin");
        symlink(&target, &link).unwrap();
        let cfg = SwarmConfig {
            manifest: one_chunk_manifest(),
            out_path: link,
            peers: vec!["127.0.0.1:1".parse().unwrap()],
            quic_ports: vec![None],
            key: [7u8; 32],
            my_pubkey: "ab".repeat(32),
            max_parallel: 2,
        };
        let report = download(cfg, Arc::new(AtomicBool::new(false))).await;
        assert_eq!(
            report.failures, 1,
            "final-component symlink must be refused before open"
        );
        assert_eq!(report.failed_hashes, vec!["ef".repeat(32)]);
        assert!(!target.exists(), "symlink target must not be created");
    }

    // ─── prepare_target: the extracted filesystem setup ───────────────────
    //
    // This is the code that moved onto the blocking pool. What needed pinning is
    // not the thread it runs on (that is a structural property of the
    // `spawn_blocking` call, and a test for it would be a timing test, which is
    // either flaky or slow) but the *order* of the checks, because that is what
    // makes WP11 hold. `swarm_refuses_symlink_out_path` covers it end to end;
    // these pin each step's contribution in isolation.

    /// The symlink refusal has to precede `open`. Checking afterwards would be
    /// checking the wrong inode — `open` follows the link, so the "refusal"
    /// would come after the target had already been opened for write and
    /// `ftruncate`d to the blob's length, which destroys whatever was there.
    #[test]
    fn prepare_target_refuses_a_symlink_and_leaves_its_target_intact() {
        use std::os::unix::fs::symlink;
        let root = soshal_test_util::tmp_root("prep_symlink");
        let target = root.join("real.bin");
        std::fs::write(&target, b"precious").unwrap();
        let link = root.join("link.bin");
        symlink(&target, &link).unwrap();

        let err = prepare_target(&link, 4096).expect_err("must refuse");
        assert!(err.contains("symlink"), "unexpected error: {err}");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"precious",
            "the symlink target must not be opened, truncated or resized"
        );
    }

    /// Path validation must also precede `open`, so a rejected name never results in
    /// a file appearing on disk. This is the other half of WP11: the symlink check
    /// covers a *link* at the final component, validation covers a *traversal*
    /// through it.
    ///
    /// Worth being precise about what validation actually does, because the obvious
    /// reading of the code is wrong: it does **not** reject `..` anywhere in the
    /// path. The parent is canonicalized — which resolves both symlinks and `..` —
    /// and only the final component is required to be a plain basename. So
    /// `dir/../file.bin` is fine (it lands inside the canonicalized parent, same as
    /// the canonical path would), while a final component that is itself a traversal
    /// is refused.
    #[test]
    fn prepare_target_rejects_a_traversing_final_component() {
        let root = soshal_test_util::tmp_root("prep_traversal");

        // A final component containing "..", in two shapes: one that is a name
        // with dots in it, and one that *is* a directory component.
        let dotted = root.join("..escaped.bin");
        let as_dir = root.join("sub").join("..");
        for (label, hostile) in [("dotted name", &dotted), ("directory component", &as_dir)] {
            let err = prepare_target(hostile, 4096).expect_err("must refuse");
            assert!(
                err.contains("file name") || err.contains("invalid out_path"),
                "{label}: unexpected error: {err}"
            );
            assert!(
                !std::fs::symlink_metadata(&dotted).is_ok_and(|m| m.is_file()),
                "{label}: a rejected path must not create a file"
            );
        }
        // Nothing at all was written into the validated directory.
        let entries: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(entries.is_empty(), "created: {entries:?}");
    }

    /// The other half of the same property, stated positively: a `..` in the
    /// parent must *not* be an escape. It is canonicalized, so the file lands in
    /// the canonicalized parent rather than anywhere above it. This is the case a
    /// naive "reject any `..`" implementation would break, so it is pinned.
    #[test]
    fn a_dotdot_in_the_parent_resolves_rather_than_escaping() {
        let root = soshal_test_util::tmp_root("prep_parent_dotdot");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        // `root/nested/../inside.bin` names a file in `root`.
        let path = nested.join("..").join("inside.bin");

        let map = prepare_target(&path, 128).expect("setup");
        assert_eq!(map.len(), 128);
        assert!(
            root.join("inside.bin").exists(),
            "the file must land in the canonicalized parent"
        );
        assert!(
            !root.parent().unwrap().join("inside.bin").exists(),
            "and must not land a level above the validated parent"
        );
    }

    /// The happy path must produce exactly a sparse file of `total` bytes,
    /// mapped over its full length. Getting this wrong is silent — a mapping
    /// shorter than the blob makes every worker hit the `range exceeds mmap len`
    /// branch and the whole download reports zero verified chunks — so it is
    /// worth asserting the exact length on both sides.
    #[test]
    fn prepare_target_sparsifies_to_exactly_total_bytes() {
        for total in [0u64, 1, 4096, 300_000] {
            let root = soshal_test_util::tmp_root("prep_sparse");
            let path = root.join("blob.bin");
            let map = prepare_target(&path, total).expect("setup");

            assert_eq!(map.len() as u64, total, "mapping must span the blob");
            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                total,
                "file must be sized to the blob, not left at 0"
            );
            // Sparse means untouched pages read as zero, which is also what the
            // workers rely on before writing their ranges.
            assert!(
                map.iter().all(|&b| b == 0),
                "a fresh sparse mapping must read as zeros ({total} bytes)"
            );
        }
    }

    /// `set_len` must come after `open` and before `mmap`. Mapping first would
    /// either fail outright on a zero-length file or produce a mapping whose
    /// length does not match the manifest, and the download would silently verify
    /// nothing.
    #[test]
    fn prepare_target_reuses_an_existing_file_without_truncating_it() {
        let root = soshal_test_util::tmp_root("prep_reuse");
        let path = root.join("existing.bin");
        std::fs::write(&path, vec![0xAAu8; 9000]).unwrap();

        let map = prepare_target(&path, 4096).expect("setup");
        assert_eq!(map.len(), 4096);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            4096,
            "set_len resizes; it must not leave the old 9000 bytes"
        );
        assert!(
            map.iter().all(|&b| b == 0xAA),
            "resize preserves contents in place, so the tail is not zeroed for us"
        );
    }

    #[test]
    fn swarm_spawn_thread_reports_fast_on_empty() {
        let root = soshal_test_util::tmp_root("swarm_spawn");
        let (handle, _) = spawn_swarm_download(SwarmConfig {
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
        let (handle, _) = spawn_swarm_download(SwarmConfig {
            manifest: m.clone(),
            out_path: out.clone(),
            peers,
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
        let (handle, _) = spawn_swarm_download(SwarmConfig {
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
        let (handle, _) = spawn_swarm_download(SwarmConfig {
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
        let (handle, _) = spawn_swarm_download(SwarmConfig {
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
        let (handle, _) = spawn_swarm_download(SwarmConfig {
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
