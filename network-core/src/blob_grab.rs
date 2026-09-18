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

use soshal_media_core::cas::ChunkStore;
use soshal_media_core::chunking::{ChunkManifest, ChunkRef};

use crate::lan_transport;
use crate::quic;

/// Upper bound on a single blob fetch (in-memory assembly). Media clips and
/// images fit; the relay stream path streams instead.
pub const MAX_BLOB_FETCH_BYTES: u64 = 512 * 1024 * 1024;

const QUIC_MAX_CHUNK: usize = 1024 * 1024;
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
    let result = (|| -> Result<u64, String> {
        let mut file =
            std::fs::File::create(out_path).map_err(|e| format!("write {out_path}: {e}"))?;
        let mut hasher = blake3::Hasher::new();
        let mut written: u64 = 0;
        let quic_addr = peer.quic_addr();
        for (batch_start, batch) in manifest.chunks.chunks(CONCURRENT_CHUNKS).enumerate() {
            let start = batch_start * CONCURRENT_CHUNKS;
            let batch_results: Vec<Result<Vec<u8>, String>> = std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(batch.len());
                for chr in batch {
                    handles.push(s.spawn(move || {
                        fetch_chunk_bytes(*peer, key, my_pubkey, quic_addr, chr.clone())
                    }));
                }
                handles
                    .into_iter()
                    .map(|h| {
                        h.join()
                            .unwrap_or_else(|_| Err("chunk thread panic".to_string()))
                    })
                    .collect()
            });
            for (bi, res) in batch_results.into_iter().enumerate() {
                let chr = &manifest.chunks[start + bi];
                let bytes = res?;
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
                if !absorbed.put_trusted(&chr.blake3, &bytes) && !absorbed.contains(&chr.blake3) {
                    return Err(format!("chunk {} store failed", chr.blake3));
                }
                file.write_all(&bytes)
                    .map_err(|e| format!("write {out_path}: {e}"))?;
                hasher.update(&bytes);
                written += bytes.len() as u64;
            }
        }

        // 3) Whole-blob verification + CAS absorb + file write.
        if hasher.finalize().to_hex().as_str() != blob_hash {
            return Err("blob hash mismatch after transfer".to_string());
        }
        absorbed
            .save_manifest(&manifest)
            .map_err(|e| format!("manifest persist: {e}"))?;
        Ok(written)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(out_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use soshal_media_core::chunking::ChunkRef;
    use std::io::{BufRead, Read, Write};

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
