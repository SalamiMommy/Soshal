//! Direct LAN peer transport: HMAC-authenticated TCP media serving.
//!
//! The server binds an ephemeral port and answers range requests for media
//! blobs stored in the local chunk CAS (verified on read). Every connection:
//!
//! 1. Must come from a private IP (loopback/1918/ULA).
//! 2. Must complete the HMAC beacon handshake — same MAC scheme the LAN
//!    broadcast beacons use, so only holders of the identity-derived key can
//!    fetch media.
//! 3. Honours the power scheduler: seeding is denied while paused (e.g.
//!    battery + cellular), so background transfers never drain the device.
//!
//! Wire format (all lengths little-endian):
//!   handshake: client → `BEACON <magic>:<pubkey>:<port>:<mac>\n` (≤512 B),
//!              server → `OK\n` | `ERR\n`
//!   request:   [u32 len][JSON LanChunkRequest]
//!   response:  [u8 kind][u32 len]\[bytes\]
//!              kind 0 = chunk bytes, 1 = not found, 2 = denied, 3 = bad request

use crate::lan;
use crate::power::global_power_scheduler;
use serde::{Deserialize, Serialize};
use soshal_media_core::cas::ChunkStore;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const LAN_MAGIC: &str = "soshal-lan";

/// Max payload a remote peer may request in one frame (one nominal chunk).
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_HANDSHAKE_LINE: usize = 512;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LanChunkRequest {
    pub hash: String,
    pub offset: usize,
    pub length: usize,
    /// When set, `hash` is a blob (manifest) hash and the peer answers with
    /// the serialized manifest JSON instead of a byte range. Lets a client
    /// discover a blob's chunk list with only its hash — the standard
    /// crawl-then-swarm flow. Offset/length are ignored in this mode.
    #[serde(default)]
    pub want_manifest: bool,
}

#[repr(u8)]
pub(crate) enum ResponseKind {
    Ok = 0,
    NotFound = 1,
    Denied = 2,
    BadRequest = 3,
}

/// Handle returned by [`start_lan_server`]; port is the bound listening port.
pub struct LanServerHandle {
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LanServerHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.take();
    }
}

/// Starts a LAN media server for the given identity-derived key, serving the
/// default chunk store.
pub fn start_lan_server(key: [u8; 32]) -> Result<LanServerHandle, String> {
    start_lan_server_with_store(key, ChunkStore::default_root())
}

/// Starts a LAN media server over an explicit chunk-store root (tests).
pub fn start_lan_server_with_store(
    key: [u8; 32],
    store_root: PathBuf,
) -> Result<LanServerHandle, String> {
    let listener = TcpListener::bind("0.0.0.0:0").map_err(|e| format!("lan bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("lan addr: {e}"))?
        .port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    const MAX_CONCURRENT_LAN_CONNS: usize = 16;
    let active_conns = Arc::new(AtomicUsize::new(0));
    let thread = std::thread::spawn(move || {
        let store = ChunkStore::new(store_root);
        for stream in listener.incoming() {
            if stop_clone.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else {
                continue;
            };
            if active_conns.load(Ordering::Relaxed) >= MAX_CONCURRENT_LAN_CONNS {
                drop(stream);
                continue;
            }
            active_conns.fetch_add(1, Ordering::SeqCst);
            let counter = active_conns.clone();
            let key = key;
            let store = store.clone();
            let _ = std::thread::spawn(move || {
                handle_conn(stream, key, &store);
                counter.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(LanServerHandle {
        port,
        stop,
        thread: Some(thread),
    })
}

fn handle_conn(stream: TcpStream, key: [u8; 32], store: &ChunkStore) {
    let Ok(peer_addr) = stream.peer_addr() else {
        return;
    };
    // Privacy invariant: LAN transfers only ever happen between private hosts.
    if !lan::is_private_ip(peer_addr.ip()) {
        return;
    }
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let peer = match stream.try_clone() {
        Ok(p) => p,
        Err(_) => return,
    };
    let mut writer = stream;
    let mut reader = BufReader::new(peer).take(MAX_HANDSHAKE_LINE as u64);

    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut inner = reader.into_inner();
    // Authenticate: MAC must match the identity-derived key...
    let now_secs = soshal_common_core::format::now_secs() as u64;
    let authed = match lan::parse_beacon(&key, LAN_MAGIC, line.trim_end(), 0, now_secs) {
        Some((pk, _, nonce)) if !global_power_scheduler().mode().paused() => {
            lan::beacon_seq().check_and_record(&pk, &nonce)
        }
        _ => false,
    };
    if authed {
        let _ = writer.write_all(b"OK\n");
    } else {
        let _ = writer.write_all(b"ERR\n");
        return;
    }

    loop {
        match read_frame(&mut inner) {
            Ok(Frame::Request(req)) => {
                let kind = if req.want_manifest {
                    serve_manifest_request(&mut writer, store, &req)
                } else {
                    serve_chunk_request(&mut writer, store, &req)
                };
                if let Some(kind) = kind {
                    let _ = write_frame(&mut writer, kind, &[]);
                }
            }
            Ok(Frame::Eof) => return,
            Err(_) => return,
        }
    }
}

/// Manifest-hash request: answer with the serialized `ChunkManifest` for the
/// blob hash so the client can build a chunk crawl without any side channel.
/// Returns `None` when a frame was already written (manifest body emitted).
fn serve_manifest_request(
    writer: &mut impl Write,
    store: &ChunkStore,
    req: &LanChunkRequest,
) -> Option<ResponseKind> {
    if req.hash.len() != 64 {
        return Some(ResponseKind::BadRequest);
    }
    if global_power_scheduler().mode().paused() {
        return Some(ResponseKind::Denied);
    }
    match store.load_manifest(&req.hash) {
        Some(m) => match serde_json::to_vec(&m) {
            Ok(bytes) if bytes.len() <= MAX_FRAME_BYTES => {
                if write_frame(writer, ResponseKind::Ok, &bytes).is_err() {
                    return None;
                }
                None
            }
            Ok(_) => Some(ResponseKind::BadRequest),
            Err(_) => Some(ResponseKind::BadRequest),
        },
        None => Some(ResponseKind::NotFound),
    }
}

/// Chunk-hash / blob-range request: resolve the owning blob either way and
/// serve the byte range (zero-copy sendfile on Linux when possible).
fn serve_chunk_request(
    writer: &mut TcpStream,
    store: &ChunkStore,
    req: &LanChunkRequest,
) -> Option<ResponseKind> {
    if req.hash.len() != 64 || req.length == 0 || req.length > MAX_FRAME_BYTES {
        return Some(ResponseKind::BadRequest);
    }
    if global_power_scheduler().mode().paused() {
        return Some(ResponseKind::Denied);
    }
    // Manifest-hash request (blob-range mode) or chunk-hash request (swarm
    // mode): resolve the owning blob either way.
    let manifest = store
        .load_manifest(&req.hash)
        .or_else(|| store.find_manifest_containing_chunk(&req.hash));
    match manifest.as_ref() {
        Some(m) => {
            // Zero-copy path first (Linux/Android): verify the covering chunk
            // once, then let the kernel pipe the chunk file straight into the
            // socket. The wire frame is identical to the copy path.
            if serve_range_zero_copy(writer, store, m, req) {
                return None;
            }
            match store.blob_slice(m, req.offset, req.length) {
                Some(bytes) => {
                    if write_frame(writer, ResponseKind::Ok, &bytes).is_err() {
                        return None;
                    }
                    None
                }
                None => Some(ResponseKind::NotFound),
            }
        }
        None => Some(ResponseKind::NotFound),
    }
}

/// Reads one length-prefixed frame from the wire.
fn read_frame(reader: &mut impl Read) -> Result<Frame, String> {
    let mut len_buf = [0u8; 4];
    read_exact_limited(reader, &mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 {
        return Ok(Frame::Eof);
    }
    if len > MAX_FRAME_BYTES {
        return Err("frame too large".to_string());
    }
    let mut payload = vec![0u8; len];
    read_exact_limited(reader, &mut payload)?;
    let req: LanChunkRequest =
        serde_json::from_slice(&payload).map_err(|_| "bad frame json".to_string())?;
    Ok(Frame::Request(req))
}

/// read_exact that treats EOF before the buffer is full as an error, and
/// bounds total reads per call so a hostile socket cannot desk the thread.
fn read_exact_limited(reader: &mut impl Read, buf: &mut [u8]) -> Result<(), String> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = reader
            .read(&mut buf[filled..])
            .map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("eof".to_string());
        }
        filled += n;
    }
    Ok(())
}

fn write_frame(writer: &mut impl Write, kind: ResponseKind, payload: &[u8]) -> Result<(), String> {
    let len_bytes = (payload.len() as u32).to_le_bytes();
    let header = [
        kind as u8,
        len_bytes[0],
        len_bytes[1],
        len_bytes[2],
        len_bytes[3],
    ];
    writer
        .write_all(&header)
        .and_then(|_| writer.write_all(payload))
        .map_err(|e| format!("write: {e}"))
}

/// Serves a range straight from the chunk file into the socket via
/// `sendfile` (kernel zero-copy: disk → NIC, no user-space buffer). The
/// covering chunk is BLAKE3-verified first; CAS chunks are immutable once
/// written (put_verified never overwrites), so verify-then-send is safe.
///
/// Returns `true` when the response should be considered served (either sent
/// fully, or the header left the socket and a sendfile failure just kills an
/// already-dead connection). Returns `false` to let the caller fall back to
/// the copy path — only for pre-header conditions: no `sendfile` on this
/// platform, multi-chunk ranges, missing/corrupt chunk, or header write
/// failure (nothing emitted, safe to retry differently).
#[cfg(target_os = "linux")]
fn serve_range_zero_copy(
    writer: &mut TcpStream,
    store: &ChunkStore,
    manifest: &soshal_media_core::chunking::ChunkManifest,
    req: &LanChunkRequest,
) -> bool {
    // The range must sit inside exactly one chunk for a single sendfile.
    let Some(end) = req.offset.checked_add(req.length) else {
        return false;
    };
    let Some(chunk) = manifest
        .chunks
        .iter()
        .find(|c| req.offset >= c.offset as usize && end <= c.offset as usize + c.len)
    else {
        return false;
    };
    let Ok(file) = std::fs::File::open(store.chunk_path(&chunk.blake3)) else {
        return false;
    };
    // BLAKE3-verify the chunk bytes before serving them: a hostile or
    // corrupted chunk file must never be streamed out unverified.
    let mut verified = Vec::new();
    if (&file).read_to_end(&mut verified).is_err()
        || blake3::hash(&verified).to_hex().as_str() != chunk.blake3
    {
        return false;
    }
    // Header first: [kind=ok][u32 len = req.length]; body bytes follow via
    // sendfile. The length prefix must match what sendfile then pipes out.
    let len_bytes = (req.length as u32).to_le_bytes();
    let header = [
        ResponseKind::Ok as u8,
        len_bytes[0],
        len_bytes[1],
        len_bytes[2],
        len_bytes[3],
    ];
    if writer.write_all(&header).is_err() {
        return false;
    }
    let mut sent = 0usize;
    let mut file_offset = (req.offset - chunk.offset as usize) as i64;
    while sent < req.length {
        let remaining = req.length - sent;
        match nix::sys::sendfile::sendfile(&writer, &file, Some(&mut file_offset), remaining) {
            Ok(0) => return true, // header already emitted; give up on the socket
            Ok(n) => sent += n,
            Err(nix::errno::Errno::EINTR) => {}
            Err(nix::errno::Errno::EAGAIN) => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_) => return true, // header already emitted; connection is dead
        }
    }
    sent == req.length
}

#[cfg(not(target_os = "linux"))]
fn serve_range_zero_copy(
    _writer: &mut TcpStream,
    _store: &ChunkStore,
    _manifest: &soshal_media_core::chunking::ChunkManifest,
    _req: &LanChunkRequest,
) -> bool {
    false
}

enum Frame {
    Request(LanChunkRequest),
    Eof,
}

/// One-shot client fetch of a blob range from a LAN peer. Only private
/// addresses are dialable (SSRF-style guard on outbound too).
pub fn fetch_chunk_range(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    req: &LanChunkRequest,
) -> Result<Vec<u8>, String> {
    if req.want_manifest {
        return Err("use fetch_manifest for manifest requests".to_string());
    }
    if req.hash.len() != 64 || req.length == 0 || req.length > MAX_FRAME_BYTES {
        return Err("bad chunk request".to_string());
    }
    let (kind, data) = lan_exchange(addr, key, my_pubkey, req)?;
    match kind {
        0 if data.len() == req.length => Ok(data),
        0 => Err("short chunk response".to_string()),
        1 => Err("chunk not found on peer".to_string()),
        2 => Err("peer seeding denied (power state)".to_string()),
        _ => Err("bad response kind".to_string()),
    }
}

/// Fetch a blob's `ChunkManifest` (JSON) from a LAN peer by blob hash. This is
/// the crawl step of a hash-only transfer: the peer resolves the manifest
/// locally and answers it directly, no side channel needed.
pub fn fetch_manifest(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    blob_hash: &str,
) -> Result<String, String> {
    if blob_hash.len() != 64 {
        return Err("invalid blob hash".to_string());
    }
    let (kind, data) = lan_exchange(
        addr,
        key,
        my_pubkey,
        &LanChunkRequest {
            hash: blob_hash.to_string(),
            offset: 0,
            length: 0,
            want_manifest: true,
        },
    )?;
    match kind {
        0 => String::from_utf8(data).map_err(|_| "manifest is not utf8".to_string()),
        1 => Err("blob manifest not found on peer".to_string()),
        2 => Err("peer seeding denied (power state)".to_string()),
        _ => Err("bad response kind".to_string()),
    }
}

/// Shared TCP exchange: HMAC beacon handshake, one JSON request frame, one
/// response frame. Returns (response kind, payload).
fn lan_exchange(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    req: &LanChunkRequest,
) -> Result<(u8, Vec<u8>), String> {
    if !lan::is_private_ip(addr.ip()) {
        return Err("refusing non-private LAN peer".to_string());
    }
    let pooled = lan_chunk_pool().take(addr);
    let was_pooled = pooled.is_some();
    let mut stream = match pooled {
        Some(s) => s,
        None => connect_handshake(addr, key, my_pubkey)?,
    };
    match exchange_frames(&mut stream, req) {
        Ok(r) => {
            lan_chunk_pool().put(addr, stream);
            Ok(r)
        }
        Err(_) if was_pooled => {
            let mut fresh = connect_handshake(addr, key, my_pubkey)?;
            let r = exchange_frames(&mut fresh, req)?;
            lan_chunk_pool().put(addr, fresh);
            Ok(r)
        }
        Err(e) => Err(e),
    }
}

fn connect_handshake(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
) -> Result<TcpStream, String> {
    let mut stream =
        TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| format!("read timeout: {e}"))?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| format!("write timeout: {e}"))?;

    // Handshake: same MAC'd beacon body the server expects.
    let ts_secs = soshal_common_core::format::now_secs() as u64;
    let nonce = hex::encode(lan::fresh_nonce());
    let body = lan::beacon_body(LAN_MAGIC, my_pubkey, 0, ts_secs, &nonce);
    let mac = lan::beacon_mac(&key, &body);
    stream
        .write_all(format!("{body}:{mac}\n").as_bytes())
        .map_err(|e| format!("handshake write: {e}"))?;

    let mut resp = [0u8; 3];
    if stream.read_exact(&mut resp).is_err() || &resp != b"OK\n" {
        return Err("handshake rejected".to_string());
    }
    Ok(stream)
}

fn exchange_frames(stream: &mut TcpStream, req: &LanChunkRequest) -> Result<(u8, Vec<u8>), String> {
    let payload = serde_json::to_vec(req).map_err(|e| format!("req serde: {e}"))?;
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|e| format!("req write: {e}"))?;

    let mut kind = [0u8; 1];
    stream
        .read_exact(&mut kind)
        .map_err(|e| format!("resp kind: {e}"))?;
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("resp len: {e}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err("oversized response".to_string());
    }
    let mut data = vec![0u8; len];
    stream
        .read_exact(&mut data)
        .map_err(|e| format!("resp body: {e}"))?;

    Ok((kind[0], data))
}

const MAX_POOLED_LAN_CONNS: usize = 16;
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

struct PooledLanConn {
    stream: TcpStream,
    last_used: std::time::Instant,
}

struct LanChunkPool {
    conns: std::sync::Mutex<std::collections::HashMap<SocketAddr, PooledLanConn>>,
}

static LAN_CHUNK_POOL: std::sync::OnceLock<LanChunkPool> = std::sync::OnceLock::new();

fn lan_chunk_pool() -> &'static LanChunkPool {
    LAN_CHUNK_POOL.get_or_init(|| LanChunkPool {
        conns: std::sync::Mutex::new(std::collections::HashMap::new()),
    })
}

impl LanChunkPool {
    fn take(&self, addr: SocketAddr) -> Option<TcpStream> {
        let mut guard = self.conns.lock().ok()?;
        let conn = guard.remove(&addr)?;
        if conn.last_used.elapsed() > POOL_IDLE_TIMEOUT {
            return None;
        }
        Some(conn.stream)
    }

    fn put(&self, addr: SocketAddr, stream: TcpStream) {
        if let Ok(mut guard) = self.conns.lock() {
            let now = std::time::Instant::now();
            guard.retain(|_, c| now.duration_since(c.last_used) <= POOL_IDLE_TIMEOUT);
            if guard.len() >= MAX_POOLED_LAN_CONNS && !guard.contains_key(&addr) {
                if let Some(oldest) = guard
                    .iter()
                    .min_by_key(|(_, c)| c.last_used)
                    .map(|(k, _)| *k)
                {
                    guard.remove(&oldest);
                }
            }
            guard.insert(
                addr,
                PooledLanConn {
                    stream,
                    last_used: now,
                },
            );
        }
    }
}

/// Reads a single chunk from a LAN peer and verifies its BLAKE3 hash.
pub fn fetch_verified_chunk(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    hash: &str,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, String> {
    if hash.len() != 64 {
        return Err("invalid chunk hash".to_string());
    }
    let data = fetch_chunk_range(
        addr,
        key,
        my_pubkey,
        &LanChunkRequest {
            hash: hash.to_string(),
            offset,
            length,
            want_manifest: false,
        },
    )?;
    if blake3::hash(&data).to_hex().as_str() != hash {
        return Err("chunk hash mismatch after transfer".to_string());
    }
    Ok(data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use soshal_media_core::cas::ChunkStore;

    #[test]
    fn beacon_handshake_roundtrip() {
        let key = [7u8; 32];
        let ts_secs = 1_700_000_000;
        let nonce = lan::fresh_nonce();
        let body = lan::beacon_body(
            LAN_MAGIC,
            &"ab".repeat(32),
            9999,
            ts_secs,
            &hex::encode(nonce),
        );
        let mac = lan::beacon_mac(&key, &body);
        let parsed = lan::parse_beacon(&key, LAN_MAGIC, &format!("{body}:{mac}"), 0, ts_secs);
        assert_eq!(parsed, Some(("ab".repeat(32), 9999, nonce)));
        let bad = lan::parse_beacon(&key, LAN_MAGIC, &format!("{body}:f00d"), 0, ts_secs);
        assert!(bad.is_none());
    }

    #[test]
    fn fetch_from_server_roundtrip() {
        let root = soshal_test_util::tmp_root("lan_test");
        let store = ChunkStore::new(root.clone());
        let data: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        let mut server = start_lan_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

        let chunk = &m.chunks[0];
        let got = fetch_verified_chunk(
            addr,
            [7u8; 32],
            &"ab".repeat(32),
            &chunk.blake3,
            0,
            chunk.len,
        )
        .unwrap();
        assert_eq!(got.len(), chunk.len);
        assert_eq!(blake3::hash(&got).to_hex().to_string(), chunk.blake3);
        server.stop();
    }

    #[test]
    fn fetch_missing_chunk_reports_not_found() {
        let root = soshal_test_util::tmp_root("lan_test");
        let mut server = start_lan_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));
        let err =
            fetch_verified_chunk(addr, [7u8; 32], &"ab".repeat(32), &"cd".repeat(32), 0, 1024)
                .unwrap_err();
        assert!(err.contains("not found"));
        server.stop();
    }

    #[test]
    fn refuses_public_peer_address() {
        let err = fetch_chunk_range(
            SocketAddr::from(([8, 8, 8, 8], 9999)),
            [7u8; 32],
            &"ab".repeat(32),
            &LanChunkRequest {
                hash: "cd".repeat(32),
                offset: 0,
                length: 16,
                want_manifest: false,
            },
        )
        .unwrap_err();
        assert!(err.contains("non-private"));
    }

    /// Test-only raw LAN server: answers every chunk request with garbage
    /// bytes (right length, wrong content) — a corrupt/hostile peer whose
    /// served data fails BLAKE3 verification.
    pub struct HostileLanServer {
        pub port: u16,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl HostileLanServer {
        pub fn stop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = self.thread.take();
        }
    }

    pub fn start_hostile_lan_server() -> HostileLanServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else {
                    continue;
                };
                let _ = std::thread::spawn(move || {
                    let Ok(peer) = stream.try_clone() else {
                        return;
                    };
                    let mut reader = BufReader::new(peer);
                    let mut writer = stream;
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let _ = writer.write_all(b"OK\n");
                    let mut inner = reader.into_inner();
                    loop {
                        match read_frame(&mut inner) {
                            Ok(Frame::Request(req)) => {
                                if req.want_manifest {
                                    let _ = write_frame(&mut writer, ResponseKind::NotFound, &[]);
                                    continue;
                                }
                                let garbage = vec![0xAAu8; req.length];
                                let _ = write_frame(&mut writer, ResponseKind::Ok, &garbage);
                            }
                            _ => return,
                        }
                    }
                });
            }
        });
        HostileLanServer {
            port,
            stop,
            thread: Some(thread),
        }
    }

    #[test]
    fn client_validation_precedes_connect() {
        let key = [7u8; 32];
        let pubkey = "ab".repeat(32);
        // 127.0.0.1:1 refuses connects; reaching it would mean validation
        // didn't fire first.
        let dead = SocketAddr::from(([127, 0, 0, 1], 1));
        let base = LanChunkRequest {
            hash: "cd".repeat(32),
            offset: 0,
            length: 16,
            want_manifest: false,
        };
        for req in [
            LanChunkRequest {
                hash: "abc".to_string(),
                ..base.clone()
            },
            LanChunkRequest {
                length: 0,
                ..base.clone()
            },
            LanChunkRequest {
                length: MAX_FRAME_BYTES + 1,
                ..base
            },
        ] {
            assert_eq!(
                fetch_chunk_range(dead, key, &pubkey, &req).unwrap_err(),
                "bad chunk request"
            );
        }
        assert_eq!(
            fetch_manifest(dead, key, &pubkey, "abc").unwrap_err(),
            "invalid blob hash"
        );
        assert_eq!(
            fetch_verified_chunk(dead, key, &pubkey, "abc", 0, 16).unwrap_err(),
            "invalid chunk hash"
        );
    }

    #[test]
    fn read_frame_guards_eof_oversize_bad_json() {
        let mut eof = &[0u8, 0, 0, 0][..];
        assert!(matches!(read_frame(&mut eof), Ok(Frame::Eof)));

        let len_bytes = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes();
        let mut over = &len_bytes[..];
        match read_frame(&mut over) {
            Err(e) => assert!(e.contains("frame too large"), "got {e}"),
            Ok(_) => panic!("expected error"),
        }

        let mut bad = &[4u8, 0, 0, 0, b'g', b'a', b'r', b'b'][..];
        match read_frame(&mut bad) {
            Err(e) => assert!(e.contains("bad frame json"), "got {e}"),
            Ok(_) => panic!("expected error"),
        }

        let mut trunc = &[5u8, 0, 0, 0, 1u8, 2u8][..];
        match read_frame(&mut trunc) {
            Err(e) => assert!(e.contains("eof"), "got {e}"),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn verified_fetch_rejects_garbage_and_copy_fallback_serves_cross_chunk() {
        let root = soshal_test_util::tmp_root("lan_test");
        let store = ChunkStore::new(root.clone());
        let data: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        // Hostile peer: right length, wrong bytes → post-transfer guard.
        let mut hostile = start_hostile_lan_server();
        let haddr = SocketAddr::from(([127, 0, 0, 1], hostile.port));
        let chunk = &m.chunks[0];
        assert_eq!(
            fetch_verified_chunk(
                haddr,
                [7u8; 32],
                &"ab".repeat(32),
                &chunk.blake3,
                0,
                chunk.len
            )
            .unwrap_err(),
            "chunk hash mismatch after transfer"
        );
        hostile.stop();

        // Honest server: cross-chunk range skips zero-copy, uses blob_slice.
        let mut server = start_lan_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));
        if m.chunks.len() >= 2 {
            let c0 = &m.chunks[0];
            let off = c0.len - 100;
            let len = 200;
            let got = fetch_chunk_range(
                addr,
                [7u8; 32],
                &"ab".repeat(32),
                &LanChunkRequest {
                    hash: m.blob_hash.clone(),
                    offset: off,
                    length: len,
                    want_manifest: false,
                },
            )
            .unwrap();
            assert_eq!(got, data[off..off + len]);
        }
        server.stop();
    }
}
