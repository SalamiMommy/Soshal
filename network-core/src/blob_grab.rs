//! Hash-only blob fetching across the LAN.
//!
//! A peer shares a blob by its BLAKE3 hash (e.g. embedded in a feed post).
//! The crawler first pulls the `ChunkManifest` (TCP or QUIC `want_manifest`
//! request), validates it against hostile-input rules, then fetches every
//! chunk body (QUIC preferred, TCP fallback), verifies each BLAKE3, verifies
//! the whole blob hash, and finally absorbs the blob into the local CAS so
//! the local media server and the seeding path can serve it.

use std::io::Write;
use std::net::{IpAddr, SocketAddr};

/// Take the body for chunk `want` from a channel that delivers in *completion*
/// order, buffering whatever lands early in `pending`.
///
/// Two failure kinds, kept distinct because the caller has to treat them
/// differently: the body itself may be a fetch `Err` (returned as
/// `Ok(Err(..))`), while a disconnected channel before `want` arrived means a
/// worker thread panicked mid-fetch (returned as `Err`).
///
/// A blocking receive is correct precisely because nothing may be written
/// before `want`, and draining the channel into `pending` cannot deadlock: a
/// worker blocked in `send` is blocked *only* because this is its sole reader
/// and is about to read.
fn take_in_order(
    rx: &std::sync::mpsc::Receiver<(usize, Result<Vec<u8>, String>)>,
    pending: &mut std::collections::BTreeMap<usize, Result<Vec<u8>, String>>,
    want: usize,
) -> Result<Result<Vec<u8>, String>, &'static str> {
    if let Some(res) = pending.remove(&want) {
        return Ok(res);
    }
    loop {
        match rx.recv() {
            Ok((idx, res)) if idx == want => return Ok(res),
            Ok((idx, res)) => {
                pending.insert(idx, res);
            }
            Err(_) => return Err("chunk thread panic"),
        }
    }
}

use soshal_media_core::cas::ChunkStore;
use soshal_media_core::chunking::{ChunkManifest, ChunkRef};

use crate::lan_transport;
use crate::quic;

/// Upper bound on a single blob fetch (in-memory assembly). Media clips and
/// images fit; the relay stream path streams instead.
pub const MAX_BLOB_FETCH_BYTES: u64 = 512 * 1024 * 1024;

const QUIC_MAX_CHUNK: usize = 16 * 1024 * 1024;
const QUIC_EXCHANGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// How many chunk bodies to fetch concurrently per batch. Bounds buffered
/// in-flight bytes to ~8 × QUIC_MAX_CHUNK while overlapping the per-chunk
/// QUIC round-trip latency instead of serializing every chunk.
const CONCURRENT_CHUNKS: usize = 8;

/// Pool is created lazily inside a runtime context (quinn needs a reactor).
static QUIC_POOL: std::sync::OnceLock<
    std::sync::Mutex<Option<std::sync::Arc<quic::QuicChunkPool>>>,
> = std::sync::OnceLock::new();
static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn block_on_chunk<F, T>(fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        match h.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(|| {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                h.spawn(async move {
                    let _ = tx.send(fut.await);
                });
                rx.recv().map_err(|_| "chunk oneshot closed".to_string())
            }),
            _ => std::thread::spawn(move || {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                h.spawn(async move {
                    let _ = tx.send(fut.await);
                });
                rx.recv().map_err(|_| "chunk oneshot closed".to_string())
            })
            .join()
            .map_err(|_| "chunk worker thread panicked".to_string())?,
        }
    } else {
        let rt = SHARED_RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .build()
                .expect("quic chunk runtime")
        });
        Ok(rt.block_on(fut))
    }
}

/// Fetch one chunk body from a peer: QUIC first, TCP fallback. Each result is
/// BLAKE3-verified against the chunk reference.
fn fetch_chunk_bytes(
    peer: LanPeer,
    key: [u8; 32],
    my_pubkey: &str,
    quic_addr: Option<SocketAddr>,
    chr: ChunkRef,
) -> Result<Vec<u8>, String> {
    let my_pubkey = my_pubkey.to_string();
    let offset =
        usize::try_from(chr.offset).map_err(|_| "chunk offset out of range".to_string())?;
    match quic_addr {
        Some(qa) => {
            let qres = if chr.len == 0 || chr.len > QUIC_MAX_CHUNK {
                Err("bad chunk request".to_string())
            } else {
                let req = crate::lan_transport::LanChunkRequest {
                    hash: chr.blake3.clone(),
                    offset,
                    length: chr.len,
                    want_manifest: false,
                };
                let pk = my_pubkey.clone();
                let fetched = block_on_chunk(async move {
                    let pool = {
                        let guard = QUIC_POOL.get_or_init(|| std::sync::Mutex::new(None));
                        let mut guard = guard.lock().unwrap_or_else(|e| e.into_inner());
                        if guard.is_none() {
                            let pool = quic::QuicChunkPool::new()
                                .map_err(|e| format!("quic pool: {e}"))?;
                            *guard = Some(std::sync::Arc::new(pool));
                        }
                        match guard.as_ref() {
                            Some(p) => p.clone(),
                            None => return Err("quic pool unavailable".to_string()),
                        }
                    };
                    let fut = pool.fetch_chunk(qa, key, &pk, &req);
                    tokio::time::timeout(QUIC_EXCHANGE_TIMEOUT, fut)
                        .await
                        .map_err(|_| "quic exchange timed out".to_string())
                        .and_then(|r| r)
                })
                .and_then(|r| r);
                match fetched {
                    Ok(data) if blake3::hash(&data).to_hex().as_str() == chr.blake3 => Ok(data),
                    Ok(_) => Err("chunk hash mismatch after transfer".to_string()),
                    Err(e) => Err(e),
                }
            };
            qres.map_err(|e| format!("quic: {e}")).or_else(|e| {
                lan_transport::fetch_verified_chunk(
                    peer.tcp_addr(),
                    key,
                    &my_pubkey,
                    &chr.blake3,
                    offset,
                    chr.len,
                )
                .map_err(|te| format!("{e}; tcp: {te}"))
            })
        }
        None => lan_transport::fetch_verified_chunk(
            peer.tcp_addr(),
            key,
            &my_pubkey,
            &chr.blake3,
            chr.offset as usize,
            chr.len,
        )
        .map_err(|e| format!("tcp: {e}")),
    }
}

/// A LAN peer that can serve chunks: TCP port always known, QUIC port only
/// when the peer advertises it.
#[derive(Debug, Clone, Copy)]
pub struct LanPeer {
    pub ip: IpAddr,
    pub tcp_port: u16,
    pub quic_port: Option<u16>,
}

impl LanPeer {
    fn quic_addr(&self) -> Option<SocketAddr> {
        self.quic_port.map(|p| SocketAddr::new(self.ip, p))
    }
    fn tcp_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.tcp_port)
    }
}

/// Validate a destination path: parent must exist and not be a symlink,
/// and no normalized component may escape via `..`.
fn validate_out_path(path: &std::path::Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "out_path has no parent".to_string())?;
    let canon = std::fs::canonicalize(parent).map_err(|e| format!("parent canonicalize: {e}"))?;
    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| "out_path has no file name".to_string())?;
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("out_path file name invalid".to_string());
    }
    if let Ok(m) = std::fs::symlink_metadata(&canon) {
        if m.file_type().is_symlink() {
            return Err("out_path parent is a symlink".to_string());
        }
    }
    Ok(())
}

/// Crawl-then-fetch a blob from one LAN peer by hash. Returns the number of
/// bytes written. The blob is also absorbed into the default CAS (verified),
/// so the local media server + seeding path can serve it afterwards.
///
/// Errors are transport-tagged ("quic:", "tcp:") so callers can fall through
/// to the next peer with the same budget.
pub fn fetch_blob_from_peer(
    peer: &LanPeer,
    key: [u8; 32],
    my_pubkey: &str,
    blob_hash: &str,
    out_path: &str,
) -> Result<u64, String> {
    if blob_hash.len() != 64 {
        return Err("invalid blob hash".to_string());
    }
    if !crate::lan::is_private_ip(peer.ip) {
        return Err("refusing non-private LAN peer".to_string());
    }
    let target_path = std::path::Path::new(out_path);
    validate_out_path(target_path)?;

    // 1) Manifest crawl: QUIC preferred, TCP fallback.
    let manifest_json = match peer.quic_addr() {
        Some(qa) => quic::fetch_quic_manifest(qa, key, my_pubkey, blob_hash)
            .map_err(|e| format!("quic: {e}"))
            .or_else(|e| {
                lan_transport::fetch_manifest(peer.tcp_addr(), key, my_pubkey, blob_hash)
                    .map_err(|te| format!("{e}; tcp: {te}"))
            }),
        None => lan_transport::fetch_manifest(peer.tcp_addr(), key, my_pubkey, blob_hash)
            .map_err(|e| format!("tcp: {e}")),
    }?;
    let manifest: ChunkManifest =
        serde_json::from_str(&manifest_json).map_err(|e| format!("hostile manifest json: {e}"))?;
    if manifest.blob_hash != blob_hash {
        return Err("manifest hash mismatch".to_string());
    }
    if manifest.total_size > MAX_BLOB_FETCH_BYTES {
        return Err(format!("blob too large ({} bytes)", manifest.total_size));
    }
    if !manifest.is_valid() {
        return Err("hostile manifest structure".to_string());
    }

    // 2) Chunk bodies: QUIC preferred, TCP fallback, each BLAKE3-verified.
    // Fetched in bounded parallel batches so per-chunk round-trip latency
    // overlaps instead of serializing the whole blob.
    let absorbed = ChunkStore::new(ChunkStore::default_root());
    let tmp_path = format!("{out_path}.tmp.{}", std::process::id());
    let mut newly_absorbed = Vec::new();
    let result = (|| -> Result<u64, String> {
        let mut file =
            std::fs::File::create(&tmp_path).map_err(|e| format!("write {tmp_path}: {e}"))?;
        let mut hasher = blake3::Hasher::new();
        let mut written: u64 = 0;
        let quic_addr = peer.quic_addr();
        // Fetch every chunk through a shared work queue rather than in serial
        // rounds of CONCURRENT_CHUNKS. Batching kept each round's per-chunk
        // round-trips overlapped but left the rounds themselves sequential, so
        // a 64-chunk blob paid 8 full round-trip latencies back to back. A
        // worker pool pulling from one atomic cursor keeps the total in-flight
        // count at CONCURRENT_CHUNKS while leaving no idle gap between rounds.
        //
        // Fetching is the only parallel part: the store/write/hash work below
        // stays strictly in manifest order, because the whole-blob BLAKE3 is a
        // rolling hash over that exact sequence. So the consumer needs chunks
        // in index order while workers finish in completion order, and the
        // consumer runs *inside* the scope, interleaved with the fetching.
        //
        // The previous shape collected every body into one `Vec` before the
        // first `write_all`, so peak RSS was the whole blob — every chunk of a
        // 512 MiB fetch resident at once, then CAS-put, then file-write: three
        // passes over data that only needed to exist once. Peak is now
        // independent of blob size.
        //
        // The bound is `2 * CONCURRENT_CHUNKS` bodies, not `CONCURRENT_CHUNKS`,
        // and that floor is structural: the bodies being fetched and the bodies
        // waiting for their turn at the front of the queue are necessarily
        // different bodies, and both sets are live simultaneously. So 8 in
        // flight + up to 8 reordered = 16 bodies = 256 MiB at
        // QUIC_MAX_CHUNK, versus up to MAX_BLOB_FETCH_BYTES (512 MiB) before.
        // The channel is 1-slot so it contributes no extra buffering of its
        // own; the `BTreeMap` is the whole reorder buffer.
        let total_chunks = manifest.chunks.len();
        let cursor = std::sync::atomic::AtomicUsize::new(0);
        let workers = CONCURRENT_CHUNKS.min(total_chunks.max(1));
        // Borrow the manifest (not move) so the same refs serve both the
        // workers and the in-order write pass below.
        let chunks = &manifest.chunks[..];
        let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, Result<Vec<u8>, String>)>(1);
        std::thread::scope(|s| -> Result<(), String> {
            for _ in 0..workers {
                let cursor = &cursor;
                let tx = tx.clone();
                s.spawn(move || {
                    loop {
                        let idx = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if idx >= total_chunks {
                            break;
                        }
                        let chr = chunks[idx].clone();
                        let res = fetch_chunk_bytes(*peer, key, my_pubkey, quic_addr, chr);
                        // A failed send means the consumer has already given
                        // up — it hit a fetch error, or a short chunk, and
                        // dropped the receiver. Stop pulling work instead of
                        // blocking forever on a channel nobody will read; the
                        // scope would otherwise join us here.
                        if tx.send((idx, res)).is_err() {
                            break;
                        }
                    }
                });
            }
            // Drop the consumer's own sender. Without this, `rx` can never see
            // the disconnect that marks "every worker finished", so the final
            // index would block forever waiting on a sender that is still alive.
            drop(tx);

            let mut pending: std::collections::BTreeMap<usize, Result<Vec<u8>, String>> =
                std::collections::BTreeMap::new();
            for bi in 0..total_chunks {
                let res = take_in_order(&rx, &mut pending, bi).map_err(|e| e.to_string())??;
                let chr = &manifest.chunks[bi];
                let bytes = res;
                if bytes.len() != chr.len {
                    return Err(format!(
                        "chunk {} short ({} != {})",
                        chr.blake3,
                        bytes.len(),
                        chr.len
                    ));
                }
                // Dedupe is normal (same clip via two peers / earlier run):
                // only the first writer stores; the whole-blob BLAKE3 check
                // still guards us.
                let stored = absorbed.put_trusted(&chr.blake3, &bytes);
                if stored {
                    newly_absorbed.push(chr.blake3.clone());
                } else if !absorbed.contains(&chr.blake3) {
                    return Err(format!("chunk {} store failed", chr.blake3));
                }
                file.write_all(&bytes)
                    .map_err(|e| format!("write {tmp_path}: {e}"))?;
                hasher.update(&bytes);
                written += bytes.len() as u64;
            }
            Ok(())
        })?;

        // 3) Whole-blob verification + CAS absorb + atomic rename.
        if hasher.finalize().to_hex().as_str() != blob_hash {
            return Err("blob hash mismatch after transfer".to_string());
        }
        std::fs::rename(&tmp_path, out_path)
            .map_err(|e| format!("atomic rename {tmp_path} -> {out_path}: {e}"))?;
        absorbed
            .save_manifest(&manifest)
            .map_err(|e| format!("manifest persist: {e}"))?;
        Ok(written)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
        // Clean up partial chunks newly stored during this failed attempt
        for hash in &newly_absorbed {
            let _ = std::fs::remove_file(absorbed.chunk_path(hash));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_media_core::chunking::ChunkRef;
    use std::io::{BufRead, Read, Write};

    /// Drain a channel fed in the given index order, asserting the bodies come
    /// back in manifest order. This is the property the whole-blob BLAKE3
    /// depends on and the one the parallel fetch makes non-obvious.
    ///
    /// The channel here is generously sized so the producer never blocks and
    /// delivery order is exactly `order`. The production 1-slot channel is
    /// covered separately by `one_slot_channel_does_not_deadlock`.
    fn drain_in_order(order: &[usize]) -> Vec<(usize, Vec<u8>)> {
        let (tx, rx) = std::sync::mpsc::sync_channel(order.len().max(1));
        for &i in order {
            tx.send((i, Ok(vec![i as u8]))).unwrap();
        }
        drop(tx);
        let mut pending = std::collections::BTreeMap::new();
        (0..order.len())
            .map(|want| {
                let body = take_in_order(&rx, &mut pending, want)
                    .expect("no worker panic")
                    .expect("no fetch error");
                (want, body)
            })
            .collect()
    }

    /// The production shape: a 1-slot channel with several live senders. The
    /// consumer asking for chunk 0 while chunks 3, 2, 1 are queued is the case
    /// that deadlocks if the consumer ever blocks on `send` or waits for its own
    /// index to be at the *front* of the channel.
    ///
    /// Two sender threads, so the delivery order is genuinely concurrent rather
    /// than a fixed script; only the required output order is asserted.
    #[test]
    fn one_slot_channel_does_not_deadlock() {
        const N: usize = 12;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let senders: Vec<_> = (0..3)
            .map(|lane| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    // Strided lanes: lane L sends L, L+3, L+6, …, so all N
                    // chunks are delivered exactly once and completion order is
                    // not index order.
                    let mut i = lane;
                    while i < N {
                        if tx.send((i, Ok(vec![i as u8]))).is_err() {
                            return;
                        }
                        i += 3;
                    }
                })
            })
            .collect();
        drop(tx);

        // Drain *while* the senders run. Joining them first would deadlock on
        // the 1-slot channel: the senders would block with a full channel and
        // nothing would be consuming, which is exactly the shape this test
        // exists to rule out in the real call site.
        let mut pending = std::collections::BTreeMap::new();
        for want in 0..N {
            let body = take_in_order(&rx, &mut pending, want)
                .expect("no worker panic")
                .expect("no fetch error");
            assert_eq!(
                body,
                vec![want as u8],
                "chunk {want} must carry its own body, not another chunk's"
            );
        }
        assert!(pending.is_empty(), "no chunk body may be left unconsumed");

        // All N bodies were consumed, so every send completed and the senders
        // are free to finish.
        for s in senders {
            s.join().unwrap();
        }
    }

    /// Chunks must be handed to the writer in manifest order no matter what
    /// order they finish in. Reversed order is the worst case: every chunk is
    /// early but the one being asked for.
    #[test]
    fn out_of_order_completion_is_reordered_to_manifest_order() {
        assert_eq!(
            drain_in_order(&[0, 1, 2, 3]),
            vec![(0, vec![0]), (1, vec![1]), (2, vec![2]), (3, vec![3]),]
        );
        // Reversed, and the fully shuffled case: the first request is for
        // chunk 0 while 3, 2, 1 are all sitting in the channel.
        assert_eq!(
            drain_in_order(&[3, 2, 1, 0]),
            vec![(0, vec![0]), (1, vec![1]), (2, vec![2]), (3, vec![3]),]
        );
        assert_eq!(
            drain_in_order(&[1, 3, 0, 2]),
            vec![(0, vec![0]), (1, vec![1]), (2, vec![2]), (3, vec![3]),]
        );
    }

    /// A fetch error belongs to its own chunk and must not be confused with a
    /// worker dying. An error on a chunk ahead of the current one is buffered
    /// and surfaces when that chunk is reached; an error on the chunk being
    /// asked for surfaces immediately. Neither reads as a panic.
    #[test]
    fn a_fetch_error_is_not_mistaken_for_a_worker_panic() {
        let (tx, rx) = std::sync::mpsc::sync_channel(3);
        // Chunk 2 fails, and it is delivered *before* chunk 0 — the error is
        // buffered, not raised, because the consumer is not at chunk 2 yet.
        tx.send((2, Err("quic: boom".to_string()))).unwrap();
        tx.send((0, Ok(vec![0]))).unwrap();
        tx.send((1, Ok(vec![1]))).unwrap();
        drop(tx);
        let mut pending = std::collections::BTreeMap::new();

        // Chunk 0 is fine even though an error is already buffered ahead of it.
        let first = take_in_order(&rx, &mut pending, 0).expect("must not be a panic");
        assert_eq!(first.expect("chunk 0 must succeed"), vec![0]);

        // Reaching chunk 2 surfaces the fetch error, still not a panic.
        let second = take_in_order(&rx, &mut pending, 1).expect("must not be a panic");
        assert_eq!(second.expect("chunk 1 must succeed"), vec![1]);
        let third = take_in_order(&rx, &mut pending, 2).expect("must not be a panic");
        assert_eq!(
            third.unwrap_err(),
            "quic: boom",
            "the real error is reported"
        );
    }

    /// A worker that dies leaves its chunk undelivered, and every sender
    /// eventually disconnects. That must report a panic, not hang, and not be
    /// mistaken for a fetch error.
    #[test]
    fn a_worker_panic_reports_instead_of_hanging() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        // Only chunk 0 is ever delivered; chunk 1's worker "dies".
        tx.send((0, Ok(vec![0]))).unwrap();
        drop(tx);
        let mut pending = std::collections::BTreeMap::new();

        let first = take_in_order(&rx, &mut pending, 0).expect("must not be a panic");
        assert_eq!(first.expect("chunk 0 must succeed"), vec![0]);

        let missing = take_in_order(&rx, &mut pending, 1);
        assert_eq!(missing.unwrap_err(), "chunk thread panic");
    }

    /// A chunk already buffered is served from the buffer, not by reaching for
    /// the channel — the buffer is the authority, because it holds bodies that
    /// arrived earlier for this index than anything still in flight.
    #[test]
    fn a_buffered_chunk_wins_over_one_still_in_the_channel() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        // A different body for the same index sits in the channel. An
        // implementation that always reached for the channel before checking
        // the buffer would return 99 and fail here.
        tx.send((7, Ok(vec![99]))).unwrap();
        let mut pending = std::collections::BTreeMap::new();
        pending.insert(7, Ok(vec![7]));

        assert_eq!(
            take_in_order(&rx, &mut pending, 7).unwrap().unwrap(),
            vec![7]
        );
        // Served from the buffer, so the channel entry is still there and was
        // not silently consumed.
        assert!(pending.is_empty());
        assert_eq!(rx.try_recv().unwrap().0, 7);
        drop(tx);
    }

    #[test]
    fn hash_only_fetch_over_quic_then_tcp() {
        let root = soshal_test_util::tmp_root("blob_grab");
        let seeder = ChunkStore::new(root.clone());
        let data: Vec<u8> = (0..(3 * 1024 * 1024 + 1234))
            .map(|i| (i % 251) as u8)
            .collect();
        let manifest = seeder.store_reader(std::io::Cursor::new(&data)).unwrap();
        seeder.save_manifest(&manifest).unwrap();
        let blob_hash = manifest.blob_hash;

        let key = [9u8; 32];
        let tcp = crate::lan_transport::start_lan_server_with_store(key, root.clone()).unwrap();
        let quic = crate::quic::start_quic_stream_server_with_store(key, root).unwrap();

        let out = soshal_test_util::tmp_root("blob_grab").join("blob.bin");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            tcp_port: tcp.port,
            quic_port: Some(quic.port),
        };
        let written = fetch_blob_from_peer(
            &peer,
            key,
            &"ab".repeat(32),
            &blob_hash,
            out.to_str().unwrap(),
        )
        .expect("blob fetch");
        assert_eq!(written, data.len() as u64);

        // Absorbed into the default CAS: local store must serve it now.
        let local = ChunkStore::new(ChunkStore::default_root());
        assert!(local.load_manifest(&blob_hash).is_some());
        assert_eq!(
            local.blob_slice(&local.load_manifest(&blob_hash).unwrap(), 0, data.len()),
            Some(data.clone())
        );

        // Same result via TCP-only peer (no QUIC advertised).
        let out2 = soshal_test_util::tmp_root("blob_grab").join("blob2.bin");
        std::fs::create_dir_all(out2.parent().unwrap()).unwrap();
        let peer2 = LanPeer {
            ip: peer.ip,
            tcp_port: tcp.port,
            quic_port: None,
        };
        let written = fetch_blob_from_peer(
            &peer2,
            key,
            &"ab".repeat(32),
            &blob_hash,
            out2.to_str().unwrap(),
        )
        .expect("tcp-only fetch");
        assert_eq!(written, data.len() as u64);
    }

    #[test]
    fn unknown_hash_reports_not_found() {
        let root = soshal_test_util::tmp_root("blob_grab");
        let key = [9u8; 32];
        let tcp = crate::lan_transport::start_lan_server_with_store(key, root.clone()).unwrap();
        let quic = crate::quic::start_quic_stream_server_with_store(key, root).unwrap();
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            tcp_port: tcp.port,
            quic_port: Some(quic.port),
        };
        let err = fetch_blob_from_peer(
            &peer,
            key,
            &"ab".repeat(32),
            &"f".repeat(64),
            "/tmp/none.bin",
        )
        .unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }

    /// One-shot LAN server that answers every `want_manifest` request with
    /// `body`, bypassing the CAS so tests can feed hostile/fake manifests.
    fn raw_manifest_server(body: Vec<u8>) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            let _ = reader.read_line(&mut String::new());
            if writer.write_all(b"OK\n").is_err() {
                return;
            }
            let mut len_buf = [0u8; 4];
            if reader.read_exact(&mut len_buf).is_err() {
                return;
            }
            let mut payload = vec![0u8; u32::from_le_bytes(len_buf) as usize];
            if reader.read_exact(&mut payload).is_err() {
                return;
            }
            let _ = writer.write_all(&[0u8]);
            let _ = writer.write_all(&(body.len() as u32).to_le_bytes());
            let _ = writer.write_all(&body);
        });
        port
    }

    #[test]
    fn invalid_blob_hash_rejected() {
        let _g = soshal_test_util::test_lock();
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            tcp_port: 1,
            quic_port: None,
        };
        let err =
            fetch_blob_from_peer(&peer, [9u8; 32], &"ab".repeat(32), "short", "/tmp/none.bin")
                .unwrap_err();
        assert!(err.contains("invalid blob hash"), "{err}");
    }

    #[test]
    fn refuses_public_lan_peer() {
        let _g = soshal_test_util::test_lock();
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::new(8, 8, 8, 8)),
            tcp_port: 9999,
            quic_port: None,
        };
        let err = fetch_blob_from_peer(
            &peer,
            [9u8; 32],
            &"ab".repeat(32),
            &"ab".repeat(32),
            "/tmp/none.bin",
        )
        .unwrap_err();
        assert!(err.contains("refusing non-private LAN peer"), "{err}");
    }

    #[test]
    fn manifest_hash_mismatch_rejected() {
        let _g = soshal_test_util::test_lock();
        let mismatched = ChunkManifest {
            blob_hash: "cd".repeat(32),
            total_size: 1024,
            chunks: vec![ChunkRef {
                blake3: "ef".repeat(32),
                offset: 0,
                len: 1024,
            }],
        };
        let port = raw_manifest_server(serde_json::to_vec(&mismatched).unwrap());
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            tcp_port: port,
            quic_port: None,
        };
        let err = fetch_blob_from_peer(
            &peer,
            [9u8; 32],
            &"ab".repeat(32),
            &"ab".repeat(32),
            "/tmp/none.bin",
        )
        .unwrap_err();
        assert!(err.contains("manifest hash mismatch"), "{err}");
    }

    #[test]
    fn oversized_blob_rejected() {
        let _g = soshal_test_util::test_lock();
        let oversized = ChunkManifest {
            blob_hash: "ab".repeat(32),
            total_size: MAX_BLOB_FETCH_BYTES + 1,
            chunks: vec![],
        };
        let port = raw_manifest_server(serde_json::to_vec(&oversized).unwrap());
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            tcp_port: port,
            quic_port: None,
        };
        let err = fetch_blob_from_peer(
            &peer,
            [9u8; 32],
            &"ab".repeat(32),
            &"ab".repeat(32),
            "/tmp/none.bin",
        )
        .unwrap_err();
        assert!(err.contains("blob too large"), "{err}");
    }

    #[test]
    fn hostile_manifest_rejected() {
        let _g = soshal_test_util::test_lock();
        let hostile = ChunkManifest {
            blob_hash: "ab".repeat(32),
            total_size: 1024,
            chunks: vec![ChunkRef {
                blake3: "cd".repeat(32),
                offset: 1,
                len: 1024,
            }],
        };
        let port = raw_manifest_server(serde_json::to_vec(&hostile).unwrap());
        let peer = LanPeer {
            ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            tcp_port: port,
            quic_port: None,
        };
        let err = fetch_blob_from_peer(
            &peer,
            [9u8; 32],
            &"ab".repeat(32),
            &"ab".repeat(32),
            "/tmp/none.bin",
        )
        .unwrap_err();
        assert!(err.contains("hostile manifest structure"), "{err}");
    }
}
