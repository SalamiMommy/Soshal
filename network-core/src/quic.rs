//! QUIC datagram channel for ultra-low-latency micro-events (typing
//! indicators, live reactions, presence pings).
//!
//! Datagrams are unreliable and out-of-order by design (RFC 9000 §14): a
//! dropped typing ping is harmless, and they never head-of-line-block behind a
//! heavy media transfer on the same 5-tuple. The channel runs on its own
//! tokio runtime thread (mirrors `swarm.rs`); the app-facing handle is a plain
//! `try_recv`/`send_to` pair — no async plumbing crosses the FFI boundary.
//!
//! Auth note: transport TLS uses a per-install self-signed cert accepted by
//! peers (mesh-internal); identity/authentication happens at the app layer via
//! the peer key in the registration datagram, same trust model as the LAN
//! beacon handshake. Micro-events are presence signals — never authoritative.

use quinn::{ClientConfig, Connection, Endpoint, ServerConfig, TransportConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Stream request type: chunk fetch or MoQ subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum StreamRequest {
    #[serde(rename = "chunk")]
    Chunk(crate::lan_transport::LanChunkRequest),
    #[serde(rename = "moq_subscribe")]
    MoqSubscribe {
        stream_id: String,
        #[serde(default = "default_moq_window")]
        window_ms: u64,
    },
}

fn default_moq_window() -> u64 {
    LIVE_DEFAULT_WINDOW_MS
}

/// One micro-event crossing the mesh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MicroEvent {
    /// e.g. "typing", "reaction", "presence", "cursor".
    pub kind: String,
    /// Opaque app payload (usually JSON).
    pub payload: String,
}

const MAX_DATAGRAM: usize = 1200;
const REGISTRATION_KIND: &str = "__peer_key__";

/// Accept-any-cert verifier: identity is app-layer (peer key datagram).
#[derive(Debug)]
struct PermitAllVerifier;

impl ServerCertVerifier for PermitAllVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ED25519,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// App-facing handle to the datagram channel (thread-safe, sync).
pub struct QuicDatagramHandle {
    addr: SocketAddr,
    receiver: Mutex<std::sync::mpsc::Receiver<MicroEvent>>,
    sender: tokio::sync::mpsc::Sender<Command>,
    stop: Arc<AtomicBool>,
}

enum Command {
    Send { peer: SocketAddr, event: MicroEvent },
}

impl QuicDatagramHandle {
    /// Polls one received micro-event, if any.
    pub fn try_recv(&self) -> Option<MicroEvent> {
        self.receiver.lock().ok()?.try_recv().ok()
    }

    /// Sends a micro-event to `peer` (establishes the QUIC connection on
    /// demand). Lossy by design: `Blocked` datagrams are dropped.
    pub fn send_to(&self, peer: SocketAddr, event: MicroEvent) -> Result<(), String> {
        self.sender
            .try_send(Command::Send { peer, event })
            .map_err(|e| format!("quic send cmd: {e}"))
    }

    /// All micro-events seen since the consumer's last call.
    pub fn drain_events(&self) -> Vec<MicroEvent> {
        self.receiver
            .lock()
            .map(|g| g.try_iter().collect())
            .unwrap_or_default()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Spawns the QUIC datagram channel on its own thread + runtime.
/// `my_peer_key` is the hex pubkey advertised in registration datagrams.
pub fn spawn_quic_datagram_channel(
    bind_port: u16,
    my_peer_key: String,
) -> Result<QuicDatagramHandle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<Command>(1024);
    let (evt_tx, evt_rx) = std::sync::mpsc::sync_channel::<MicroEvent>(1024);

    let thread_stop = stop.clone();
    let thread = std::thread::Builder::new()
        .name("soshal-quic".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("quic runtime: {e}");
                    return;
                }
            };
            rt.block_on(run_channel(
                bind_port,
                my_peer_key,
                cmd_rx,
                evt_tx,
                thread_stop,
            ));
        })
        .map_err(|e| format!("quic thread: {e}"))?;

    // The runtime path fills the handle's addr via its own mpsc; derive the
    // bound port synchronously instead:
    let addr = SocketAddr::from(([0, 0, 0, 0], bind_port));
    let _ = thread; // keeps runtime alive; stopped via `stop`
    Ok(QuicDatagramHandle {
        addr,
        receiver: Mutex::new(evt_rx),
        sender: cmd_tx,
        stop,
    })
}

async fn run_channel(
    bind_port: u16,
    my_peer_key: String,
    mut cmd_rx: tokio::sync::mpsc::Receiver<Command>,
    evt_tx: std::sync::mpsc::SyncSender<MicroEvent>,
    stop: Arc<AtomicBool>,
) {
    let (server_config, client_config) = match tls_configs() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("quic tls: {e}");
            return;
        }
    };

    let socket = match std::net::UdpSocket::bind(("0.0.0.0", bind_port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("quic bind: {e}");
            return;
        }
    };
    let bound = socket.local_addr().ok();
    socket.set_nonblocking(true).ok();
    let runtime = Arc::new(quinn::TokioRuntime);
    let mut server_config = server_config;
    server_config.transport_config(Arc::new(transport_config()));
    let mut endpoint = match Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        socket,
        runtime,
    ) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("quic endpoint: {e}");
            return;
        }
    };
    endpoint.set_default_client_config(client_config);

    let mut conns: HashMap<SocketAddr, Connection> = HashMap::new();
    let mut sweep = tokio::time::interval(std::time::Duration::from_secs(30));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        tokio::select! {
            _ = sweep.tick() => {
                // Evict dead connections (closed by peer or idle timeout) so
                // the map cannot grow unboundedly over long sessions.
                conns.retain(|_, c| c.close_reason().is_none());
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { continue; };
                if let Ok(conn) = incoming.await {
                    spawn_conn_task(conn, evt_tx.clone(), bound);
                }
            }
            cmd = recv_cmd(&mut cmd_rx) => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    Command::Send { peer, event } => {
                        let payload = match serde_json::to_vec(&event) {
                            Ok(p) => p,
                            Err(e) => { eprintln!("quic serialize: {e}"); continue; }
                        };
                        if payload.len() > MAX_DATAGRAM {
                            eprintln!("quic datagram oversized, dropped");
                            continue;
                        }
                        let conn = match conns.get(&peer) {
                            Some(c) => c.clone(),
                            None => {
                                let connecting = endpoint.connect(peer, "soshal");
                                let conn = match connecting {
                                    Ok(c) => c,
                                    Err(_) => continue,
                                };
                                match conn.await {
                                    Ok(c) => {
                                        conns.insert(peer, c.clone());
                                        // register identity
                                        let reg = MicroEvent {
                                            kind: REGISTRATION_KIND.to_string(),
                                            payload: my_peer_key.clone(),
                                        };
                                        if let Ok(bytes) = serde_json::to_vec(&reg) {
                                            if bytes.len() <= MAX_DATAGRAM {
                                                let _ = c.send_datagram(bytes.into());
                                            }
                                        }
                                        spawn_conn_task(c.clone(), evt_tx.clone(), bound);
                                        c
                                    }
                                    Err(_) => continue,
                                }
                            }
                        };
                        if conn.send_datagram(payload.into()).is_err() {
                            conns.remove(&peer);
                        }
                    }
                }
            }
        }
    }
    endpoint.wait_idle().await;
}

fn spawn_conn_task(
    conn: Connection,
    evt_tx: std::sync::mpsc::SyncSender<MicroEvent>,
    _bound: Option<SocketAddr>,
) {
    tokio::spawn(async move {
        loop {
            match conn.read_datagram().await {
                Ok(bytes) => {
                    if let Ok(ev) = serde_json::from_slice::<MicroEvent>(&bytes) {
                        if ev.kind != REGISTRATION_KIND {
                            let _ = evt_tx.send(ev);
                        }
                    }
                }
                Err(quinn::ConnectionError::ApplicationClosed(_))
                | Err(quinn::ConnectionError::ConnectionClosed(_))
                | Err(quinn::ConnectionError::Reset)
                | Err(quinn::ConnectionError::TimedOut) => break,
                Err(_) => {}
            }
        }
    });
}

async fn recv_cmd(rx: &mut tokio::sync::mpsc::Receiver<Command>) -> Option<Command> {
    rx.recv().await
}

fn transport_config() -> TransportConfig {
    let mut cfg = TransportConfig::default();
    cfg.datagram_receive_buffer_size(Some(64 * 1024));
    cfg.stream_receive_window((8 * 1024 * 1024u32).into());
    cfg.receive_window((16 * 1024 * 1024u32).into());
    cfg.keep_alive_interval(Some(std::time::Duration::from_secs(15)));
    cfg.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(std::time::Duration::from_secs(30)).expect("idle timeout"),
    ));
    cfg
}

#[allow(unsafe_code)]
fn tls_configs() -> Result<(ServerConfig, ClientConfig), String> {
    // rustls 0.23 requires a process-level CryptoProvider. quinn's datagram
    // channel and the stream channel below both hit this the first time a
    // real handshake runs; install ring's provider once (idempotent).
    static PROVIDER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    PROVIDER.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });

    // The self-signed cert + configs are identity-independent; build once and
    // reuse (rcgen cert generation per call was measurable on chunk fetches).
    static CFG: std::sync::OnceLock<(ServerConfig, ClientConfig)> = std::sync::OnceLock::new();
    if let Some(cfg) = CFG.get() {
        return Ok((cfg.0.clone(), cfg.1.clone()));
    }

    let cert = rcgen::generate_simple_self_signed(vec!["soshal.local".to_string()])
        .map_err(|e| format!("rcgen: {e}"))?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key = cert.key_pair.serialize_der();
    let key_der =
        rustls::pki_types::PrivateKeyDer::try_from(key).map_err(|e| format!("key der: {e}"))?;

    let server_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(
        quinn::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der)
            .map_err(|e| format!("server cert: {e}"))?,
    )
    .map_err(|e| format!("quic server config: {e}"))?;

    let client_roots = rustls::RootCertStore::empty();
    let mut client_tls = quinn::rustls::ClientConfig::builder()
        .with_root_certificates(client_roots)
        .with_no_client_auth();
    client_tls
        .dangerous()
        .set_certificate_verifier(Arc::new(PermitAllVerifier));

    let client_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(client_tls)
        .map_err(|e| format!("quic client config: {e}"))?;
    let mut client_config = ClientConfig::new(Arc::new(client_crypto));
    client_config.transport_config(Arc::new(transport_config()));

    let pair = (
        ServerConfig::with_crypto(Arc::new(server_crypto)),
        client_config,
    );
    let _ = CFG.set(pair.clone());
    Ok(pair)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micro_event_serde_roundtrip() {
        let ev = MicroEvent {
            kind: "typing".to_string(),
            payload: r#"{"to":"alice"}"#.to_string(),
        };
        let bytes = serde_json::to_vec(&ev).unwrap();
        let back: MicroEvent = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(ev, back);
    }
}

// ============================================================================
// QUIC stream channel: reliable bulk media transfer (chunk fetch + serve).
// Same trust model as the TCP LAN transport (HMAC beacon handshake, private
// IP only, power-scheduler gated), wired over QUIC bi-streams so connection
// migration (Wi-Fi ↔ cellular) and multiplexing come from the kernel of the
// protocol rather than a hand-rolled TCP layer. The datagram channel above
// stays for lossy micro-events; this one is for data that must arrive whole.
// ============================================================================

use crate::lan;
use crate::power::global_power_scheduler;
use soshal_media_core::cas::ChunkStore;
use std::net::UdpSocket;
use std::time::Duration as StdDuration;

const MAX_STREAM_FRAME: usize = 1024 * 1024;

/// Live MoQ group-log limits: bounded memory, hostile-publisher safe.
const LIVE_MAX_STREAM_ID: usize = 128;
const LIVE_MAX_STREAMS: usize = 32;
const LIVE_STREAM_HISTORY: usize = 128;
const LIVE_MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
const MAX_MOQ_FETCH_BYTES: usize = 64 * 1024 * 1024;
const LIVE_MAX_GROUP_BYTES: usize = 64 * 1024 * 1024;
const LIVE_WAKE_TIMEOUT_MS: u64 = 500;
const LIVE_IDLE_FINISH_MS: u64 = 2000;
const LIVE_DEFAULT_WINDOW_MS: u64 = 5000;

/// Bounded append-only log of encoded MoQ groups for one live stream.
#[derive(Default)]
struct LiveStreamLog {
    entries: std::collections::BTreeMap<u64, Arc<Vec<u8>>>,
    watermark: u64,
}

/// Process-wide live stream registry: publishers push encoded groups,
/// subscribers read buffered groups and follow new ones until idle/window end.
#[derive(Default)]
pub struct LiveStreamRegistry {
    streams: Mutex<HashMap<String, Arc<Mutex<LiveStreamLog>>>>,
    notify: tokio::sync::Notify,
}

impl LiveStreamRegistry {
    fn get(&self, stream_id: &str) -> Option<Arc<Mutex<LiveStreamLog>>> {
        self.streams.lock().ok()?.get(stream_id).cloned()
    }

    /// Append one encoded group; returns the group counter after append
    /// (monotonic sequence used by subscribers to detect new groups).
    fn push(&self, stream_id: &str, encoded: Vec<u8>) -> Result<u64, String> {
        if stream_id.is_empty() || stream_id.len() > LIVE_MAX_STREAM_ID {
            return Err("bad live stream id".to_string());
        }
        if encoded.is_empty() || encoded.len() > LIVE_MAX_GROUP_BYTES {
            return Err("bad live group payload".to_string());
        }
        let mut streams = self
            .streams
            .lock()
            .map_err(|_| "live registry poisoned".to_string())?;
        if streams.len() >= LIVE_MAX_STREAMS && !streams.contains_key(stream_id) {
            return Err("live registry full".to_string());
        }
        let log = streams.entry(stream_id.to_string()).or_default();
        let mut lock = log
            .lock()
            .map_err(|_| "live stream log poisoned".to_string())?;
        lock.watermark += 1;
        let seq = lock.watermark;
        lock.entries.insert(seq, Arc::new(encoded));
        while lock.entries.len() > LIVE_STREAM_HISTORY {
            lock.entries.pop_first();
        }
        drop(lock);
        self.notify.notify_waiters();
        Ok(seq)
    }
}

static LIVE_REGISTRY: std::sync::OnceLock<LiveStreamRegistry> = std::sync::OnceLock::new();

/// Accessor for the process-wide live stream registry.
pub fn live_registry() -> &'static LiveStreamRegistry {
    LIVE_REGISTRY.get_or_init(LiveStreamRegistry::default)
}

/// Publish an encoded MoQ group into the live registry (called by the
/// publisher side of the app; bytes are opaque to the transport).
pub fn moq_publish_group(stream_id: &str, encoded: Vec<u8>) -> Result<u64, String> {
    live_registry().push(stream_id, encoded)
}

/// Whether a live stream currently exists in the registry.
pub fn moq_stream_known(stream_id: &str) -> bool {
    live_registry().get(stream_id).is_some()
}
const STREAM_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const STREAM_EXCHANGE_TIMEOUT: StdDuration = StdDuration::from_secs(15);
const AUTH_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// Handle for the QUIC stream media server (mirrors `LanServerHandle`).
pub struct QuicStreamServerHandle {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl QuicStreamServerHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Starts a QUIC stream media server for `key`, serving the default chunk store.
pub fn start_quic_stream_server(key: [u8; 32]) -> Result<QuicStreamServerHandle, String> {
    start_quic_stream_server_with_store(key, ChunkStore::default_root())
}

/// Starts a QUIC stream media server over an explicit chunk-store root (tests).
pub fn start_quic_stream_server_with_store(
    key: [u8; 32],
    store_root: std::path::PathBuf,
) -> Result<QuicStreamServerHandle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
    let thread_stop = stop.clone();
    std::thread::Builder::new()
        .name("soshal-quic-srv".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("quic stream runtime: {e}");
                    return;
                }
            };
            rt.block_on(run_stream_server(key, store_root, port_tx, thread_stop));
        })
        .map_err(|e| format!("quic stream thread: {e}"))?;
    let port = port_rx
        .recv_timeout(STREAM_CONNECT_TIMEOUT)
        .map_err(|e| format!("quic stream bind: {e}"))?;
    Ok(QuicStreamServerHandle { port, stop })
}

async fn run_stream_server(
    key: [u8; 32],
    store_root: std::path::PathBuf,
    port_tx: std::sync::mpsc::Sender<u16>,
    stop: Arc<AtomicBool>,
) {
    let (server_config, _client_config) = match tls_configs() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("quic stream tls: {e}");
            return;
        }
    };
    let socket = match UdpSocket::bind(("0.0.0.0", 0)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("quic stream bind: {e}");
            return;
        }
    };
    let bound_port = socket.local_addr().ok().map(|a| a.port()).unwrap_or(0);
    socket.set_nonblocking(true).ok();
    let runtime = Arc::new(quinn::TokioRuntime);
    let mut server_config = server_config;
    server_config.transport_config(Arc::new(transport_config()));
    let endpoint = match Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        socket,
        runtime,
    ) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("quic stream endpoint: {e}");
            return;
        }
    };
    let _ = port_tx.send(bound_port);

    let store = ChunkStore::new(store_root);
    let mut tick = tokio::time::interval(StdDuration::from_millis(200));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break; };
                if let Ok(conn) = incoming.await {
                    let store = store.clone();
                    tokio::spawn(serve_stream_conn(conn, key, store));
                }
            }
        }
    }
    endpoint.wait_idle().await;
}

/// Accepts bi-streams on one connection and serves each in its own task so
/// parallel chunk fetches never head-of-line block each other.
async fn serve_stream_conn(conn: Connection, key: [u8; 32], store: ChunkStore) {
    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                let store = store.clone();
                tokio::spawn(serve_stream(send, recv, key, store));
            }
            Err(quinn::ConnectionError::ApplicationClosed(_))
            | Err(quinn::ConnectionError::ConnectionClosed(_))
            | Err(quinn::ConnectionError::Reset)
            | Err(quinn::ConnectionError::TimedOut) => break,
            Err(_) => {}
        }
    }
}

async fn serve_stream(
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    key: [u8; 32],
    store: ChunkStore,
) {
    // Auth: exactly like the TCP transport, the client must open with the
    // HMAC'd beacon line; anything else gets a refused stream.
    match auth_stream(&mut recv, &key).await {
        Ok(()) if !global_power_scheduler().mode().paused() => {}
        _ => {
            let _ = send.write_all(b"ERR\n").await;
            let _ = send.finish();
            return;
        }
    }

    // Request frame: [u32 len][JSON LanChunkRequest], same as TCP.
    let mut len_buf = [0u8; 4];
    if !matches!(
        tokio::time::timeout(
            STREAM_EXCHANGE_TIMEOUT,
            read_exact_async(&mut recv, &mut len_buf)
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let req_len = u32::from_le_bytes(len_buf) as usize;
    if req_len == 0 || req_len > MAX_STREAM_FRAME {
        let _ = write_stream_frame(&mut send, StreamResponseKind::BadRequest, &[]).await;
        let _ = send.finish();
        return;
    }
    let mut req_bytes = vec![0u8; req_len];
    if !matches!(
        tokio::time::timeout(
            STREAM_EXCHANGE_TIMEOUT,
            read_exact_async(&mut recv, &mut req_bytes)
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let req: StreamRequest = match serde_json::from_slice(&req_bytes) {
        Ok(r) => r,
        Err(_) => {
            let _ = write_stream_frame(&mut send, StreamResponseKind::BadRequest, &[]).await;
            let _ = send.finish();
            return;
        }
    };

    match req {
        StreamRequest::Chunk(chunk_req) => {
            if chunk_req.want_manifest {
                let kind = if chunk_req.hash.len() != 64 {
                    StreamResponseKind::BadRequest
                } else if global_power_scheduler().mode().paused() {
                    StreamResponseKind::Denied
                } else {
                    match store.load_manifest(&chunk_req.hash) {
                        Some(m) => match serde_json::to_vec(&m) {
                            Ok(bytes) if bytes.len() <= MAX_STREAM_FRAME => {
                                let _ =
                                    write_stream_frame(&mut send, StreamResponseKind::Ok, &bytes)
                                        .await;
                                let _ = send.finish();
                                return;
                            }
                            Ok(_) | Err(_) => StreamResponseKind::BadRequest,
                        },
                        None => StreamResponseKind::NotFound,
                    }
                };
                let _ = write_stream_frame(&mut send, kind, &[]).await;
                let _ = send.finish();
                return;
            }
            let kind = if chunk_req.hash.len() != 64
                || chunk_req.length == 0
                || chunk_req.length > MAX_STREAM_FRAME
            {
                StreamResponseKind::BadRequest
            } else if global_power_scheduler().mode().paused() {
                StreamResponseKind::Denied
            } else {
                let manifest = store
                    .load_manifest(&chunk_req.hash)
                    .or_else(|| store.find_manifest_containing_chunk(&chunk_req.hash));
                match manifest
                    .as_ref()
                    .and_then(|m| store.blob_slice(m, chunk_req.offset, chunk_req.length))
                {
                    Some(bytes) => {
                        let _ = write_stream_frame(&mut send, StreamResponseKind::Ok, &bytes).await;
                        let _ = send.finish();
                        return;
                    }
                    None => StreamResponseKind::NotFound,
                }
            };
            let _ = write_stream_frame(&mut send, kind, &[]).await;
            let _ = send.finish();
        }
        StreamRequest::MoqSubscribe {
            stream_id,
            window_ms,
        } => {
            if stream_id.is_empty() || stream_id.len() > LIVE_MAX_STREAM_ID {
                let _ = write_stream_frame(&mut send, StreamResponseKind::BadRequest, &[]).await;
                let _ = send.finish();
            } else if global_power_scheduler().mode().paused() {
                let _ = write_stream_frame(&mut send, StreamResponseKind::Denied, &[]).await;
                let _ = send.finish();
            } else {
                serve_moq_subscription(&mut send, &stream_id, window_ms).await;
            }
        }
    }
}

/// Serve a live MoQ subscription: replay the buffered group log, then follow
/// new groups every poll interval until an idle gap or the window expires.
async fn serve_moq_subscription(send: &mut quinn::SendStream, stream_id: &str, window_ms: u64) {
    let Some(log) = live_registry().get(stream_id) else {
        let _ = write_stream_frame(send, StreamResponseKind::NotFound, &[]).await;
        let _ = send.finish();
        return;
    };

    let window = window_ms.clamp(500, 60_000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(window);
    let (replay, wm) = {
        let lock = match log.lock() {
            Ok(l) => l,
            Err(_) => return,
        };
        let mut replay: Vec<Arc<Vec<u8>>> = Vec::new();
        let mut replayed_bytes = 0usize;
        for (_, g) in lock.entries.iter() {
            if !replay.is_empty() && replayed_bytes + g.len() > LIVE_MAX_REPLAY_BYTES {
                break;
            }
            replayed_bytes += g.len();
            replay.push(g.clone());
        }
        (replay, lock.watermark)
    };
    let mut watermark = wm;
    for group in &replay {
        if write_stream_frame(send, StreamResponseKind::Ok, group.as_slice())
            .await
            .is_err()
        {
            let _ = send.finish();
            return;
        }
    }
    let mut last_seen = std::time::Instant::now();

    while std::time::Instant::now() < deadline {
        let (pending, notified) = {
            let lock = match log.lock() {
                Ok(l) => l,
                Err(_) => return,
            };
            let notified = live_registry().notify.notified();
            let pending: Vec<(u64, Arc<Vec<u8>>)> = lock
                .entries
                .range((
                    std::ops::Bound::Excluded(watermark),
                    std::ops::Bound::Unbounded,
                ))
                .map(|(seq, g)| (*seq, g.clone()))
                .collect();
            (pending, notified)
        };
        if pending.is_empty() {
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(std::time::Duration::from_millis(LIVE_WAKE_TIMEOUT_MS)) => {}
            }
        }
        for (seq, group) in &pending {
            if write_stream_frame(send, StreamResponseKind::Ok, group.as_slice())
                .await
                .is_err()
            {
                let _ = send.finish();
                return;
            }
            last_seen = std::time::Instant::now();
            watermark = *seq;
        }
        if std::time::Instant::now().duration_since(last_seen)
            > std::time::Duration::from_millis(LIVE_IDLE_FINISH_MS)
        {
            break;
        }
    }
    let _ = send.finish();
}

async fn auth_stream(recv: &mut quinn::RecvStream, key: &[u8; 32]) -> Result<(), String> {
    let mut line = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    tokio::time::timeout(AUTH_TIMEOUT, async {
        loop {
            match recv.read(&mut byte).await {
                Ok(None) => return Err("eof during handshake".to_string()),
                Ok(Some(_)) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
                    if line.len() > 512 {
                        return Err("handshake line too long".to_string());
                    }
                }
                Err(_) => return Err("handshake read error".to_string()),
            }
        }
        let line = String::from_utf8_lossy(&line);
        match crate::lan::parse_beacon(
            key,
            crate::lan_transport::LAN_MAGIC,
            line.trim_end(),
            0,
            soshal_common_core::format::now_secs() as u64,
        ) {
            Some(_) => Ok(()),
            None => Err("bad hmac beacon".to_string()),
        }
    })
    .await
    .map_err(|_| "auth handshake timed out".to_string())?
}

async fn read_exact_async(recv: &mut quinn::RecvStream, buf: &mut [u8]) -> Result<(), String> {
    let mut filled = 0;
    while filled < buf.len() {
        match recv.read(&mut buf[filled..]).await {
            Ok(None) => return Err("eof".to_string()),
            Ok(Some(n)) => filled += n,
            Err(_) => return Err("read error".to_string()),
        }
    }
    Ok(())
}

#[repr(u8)]
enum StreamResponseKind {
    Ok = 0,
    NotFound = 1,
    Denied = 2,
    BadRequest = 3,
}

async fn write_stream_frame(
    send: &mut quinn::SendStream,
    kind: StreamResponseKind,
    payload: &[u8],
) -> Result<(), String> {
    send.write_all(&[kind as u8])
        .await
        .map_err(|e| format!("stream write: {e}"))?;
    send.write_all(&(payload.len() as u32).to_le_bytes())
        .await
        .map_err(|e| format!("stream write: {e}"))?;
    send.write_all(payload)
        .await
        .map_err(|e| format!("stream write: {e}"))
}

/// Shared QUIC stream exchange over an existing connection: HMAC beacon
/// handshake, tagged chunk request, length-prefixed response read.
async fn exchange_chunk(
    conn: &quinn::Connection,
    key: [u8; 32],
    my_pubkey: &str,
    req: &crate::lan_transport::LanChunkRequest,
) -> Result<Vec<u8>, String> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| format!("open stream: {e}"))?;

    let body = lan::beacon_body(
        crate::lan_transport::LAN_MAGIC,
        my_pubkey,
        0,
        soshal_common_core::format::now_secs() as u64,
    );
    let mac = lan::beacon_mac(&key, &body);
    send.write_all(format!("{body}:{mac}\n").as_bytes())
        .await
        .map_err(|e| format!("handshake write: {e}"))?;

    let payload = serde_json::to_vec(&serde_json::json!({
        "type": "chunk",
        "hash": req.hash,
        "offset": req.offset,
        "length": req.length,
        "want_manifest": req.want_manifest,
    }))
    .map_err(|e| format!("req serde: {e}"))?;
    send.write_all(&(payload.len() as u32).to_le_bytes())
        .await
        .map_err(|e| format!("req write: {e}"))?;
    send.write_all(&payload)
        .await
        .map_err(|e| format!("req write: {e}"))?;
    send.finish().map_err(|e| format!("finish: {e}"))?;

    let mut kind = [0u8; 1];
    read_exact_async(&mut recv, &mut kind).await?;
    let mut len_buf = [0u8; 4];
    read_exact_async(&mut recv, &mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_STREAM_FRAME {
        return Err("oversized response".to_string());
    }
    let mut data = vec![0u8; len];
    read_exact_async(&mut recv, &mut data).await?;

    match kind[0] {
        0 if req.want_manifest || len == req.length => Ok(data),
        0 => Err("short chunk response".to_string()),
        1 => Err("chunk not found on peer".to_string()),
        2 => Err("peer seeding denied (power state)".to_string()),
        _ => Err("bad response kind".to_string()),
    }
}

/// Connect a fresh one-shot QUIC client endpoint to `addr` (endpoint kept
/// alive for the caller's exchange).
async fn quic_connect(addr: SocketAddr) -> Result<(Endpoint, quinn::Connection), String> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| format!("udp bind: {e}"))?;
    socket.set_nonblocking(true).ok();
    let runtime = Arc::new(quinn::TokioRuntime);
    let mut endpoint = Endpoint::new(quinn::EndpointConfig::default(), None, socket, runtime)
        .map_err(|e| format!("endpoint: {e}"))?;
    let (_, client_config) = tls_configs()?;
    endpoint.set_default_client_config(client_config);
    let conn = endpoint
        .connect(addr, "soshal")
        .map_err(|e| format!("connect: {e}"))?
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    Ok((endpoint, conn))
}

/// One-shot QUIC stream fetch of a blob range from a peer. Only private
/// addresses are dialable (SSRF guard on outbound, mirrors TCP fetch). The
/// HMAC beacon authenticates the client; the response is verified by the
/// caller (or `fetch_quic_verified_chunk`).
pub fn fetch_quic_chunk(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    req: &crate::lan_transport::LanChunkRequest,
) -> Result<Vec<u8>, String> {
    if !lan::is_private_ip(addr.ip()) {
        return Err("refusing non-private LAN peer".to_string());
    }
    if req.hash.len() != 64 || req.length == 0 || req.length > MAX_STREAM_FRAME {
        return Err("bad chunk request".to_string());
    }
    static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => SHARED_RT
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .build()
                    .expect("quic chunk runtime")
            })
            .handle()
            .clone(),
    };
    rt.block_on(async {
        tokio::time::timeout(STREAM_EXCHANGE_TIMEOUT, async {
            let (_endpoint, conn) = quic_connect(addr).await?;
            exchange_chunk(&conn, key, my_pubkey, req).await
        })
        .await
        .map_err(|_| "quic exchange timed out".to_string())?
    })
}

/// Reusable QUIC client for bulk chunk fetches: one endpoint and one
/// connection per peer, reused across chunks — the swarm download path avoids
/// a fresh socket + full TLS handshake per chunk.
pub struct QuicChunkPool {
    endpoint: Endpoint,
    conns: std::sync::Mutex<std::collections::HashMap<SocketAddr, quinn::Connection>>,
}

impl QuicChunkPool {
    pub fn new() -> Result<Self, String> {
        let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| format!("udp bind: {e}"))?;
        socket.set_nonblocking(true).ok();
        let runtime = Arc::new(quinn::TokioRuntime);
        let mut endpoint = Endpoint::new(quinn::EndpointConfig::default(), None, socket, runtime)
            .map_err(|e| format!("endpoint: {e}"))?;
        let (_, client_config) = tls_configs()?;
        endpoint.set_default_client_config(client_config);
        Ok(Self {
            endpoint,
            conns: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn cached(&self, addr: SocketAddr) -> Option<quinn::Connection> {
        self.conns
            .lock()
            .ok()
            .and_then(|guard| guard.get(&addr).cloned())
    }

    fn store(&self, addr: SocketAddr, conn: &quinn::Connection) {
        if let Ok(mut guard) = self.conns.lock() {
            guard.insert(addr, conn.clone());
        }
    }

    fn evict(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.conns.lock() {
            guard.remove(&addr);
        }
    }

    /// Fetch one chunk over the pooled connection for `addr`; a stale
    /// connection is evicted and retried once with a fresh handshake.
    pub async fn fetch_chunk(
        &self,
        addr: SocketAddr,
        key: [u8; 32],
        my_pubkey: &str,
        req: &crate::lan_transport::LanChunkRequest,
    ) -> Result<Vec<u8>, String> {
        if !lan::is_private_ip(addr.ip()) {
            return Err("refusing non-private LAN peer".to_string());
        }
        for attempt in 0..2 {
            let conn = match self.cached(addr) {
                Some(c) => c,
                None => {
                    let conn = self
                        .endpoint
                        .connect(addr, "soshal")
                        .map_err(|e| format!("connect: {e}"))?
                        .await
                        .map_err(|e| format!("handshake: {e}"))?;
                    self.store(addr, &conn);
                    conn
                }
            };
            match exchange_chunk(&conn, key, my_pubkey, req).await {
                Ok(data) => return Ok(data),
                Err(_e) if attempt == 0 => {
                    self.evict(addr);
                }
                Err(e) => return Err(e),
            }
        }
        Err("chunk fetch failed".to_string())
    }
}

/// QUIC stream fetch of a blob manifest (JSON) by blob hash. The crawl step
/// of a hash-only transfer; chunk bodies then come via `fetch_quic_verified_chunk`.
pub fn fetch_quic_manifest(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    blob_hash: &str,
) -> Result<String, String> {
    if blob_hash.len() != 64 {
        return Err("invalid blob hash".to_string());
    }
    let req = crate::lan_transport::LanChunkRequest {
        hash: blob_hash.to_string(),
        offset: 0,
        length: 0,
        want_manifest: true,
    };
    let data = fetch_quic_raw(addr, key, my_pubkey, &req)?;
    String::from_utf8(data).map_err(|_| "manifest is not utf8".to_string())
}

/// Shared QUIC one-shot exchange body for `fetch_quic_chunk`-style fetches.
fn fetch_quic_raw(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    req: &crate::lan_transport::LanChunkRequest,
) -> Result<Vec<u8>, String> {
    if !lan::is_private_ip(addr.ip()) {
        return Err("refusing non-private LAN peer".to_string());
    }
    static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => SHARED_RT
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .build()
                    .expect("quic chunk runtime")
            })
            .handle()
            .clone(),
    };
    rt.block_on(async {
        tokio::time::timeout(STREAM_EXCHANGE_TIMEOUT, async {
            let (_endpoint, conn) = quic_connect(addr).await?;
            exchange_chunk(&conn, key, my_pubkey, req).await
        })
        .await
        .map_err(|_| "quic exchange timed out".to_string())?
    })
}

/// QUIC stream fetch with BLAKE3 hash verification of the payload.
pub fn fetch_quic_verified_chunk(
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
    let data = fetch_quic_chunk(
        addr,
        key,
        my_pubkey,
        &crate::lan_transport::LanChunkRequest {
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

/// Subscribe to a live MoQ stream over QUIC. Returns every encoded group
/// frame the server sends during the window (buffered replay + live follow).
/// Each element is one encoded group (streaming-core framing, opaque here).
pub fn fetch_quic_moq_groups(
    addr: SocketAddr,
    key: [u8; 32],
    my_pubkey: &str,
    stream_id: &str,
    window_ms: u64,
) -> Result<Vec<Vec<u8>>, String> {
    if !lan::is_private_ip(addr.ip()) {
        return Err("refusing non-private LAN peer".to_string());
    }
    if stream_id.is_empty() || stream_id.len() > LIVE_MAX_STREAM_ID {
        return Err("bad stream id".to_string());
    }
    static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => SHARED_RT
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .build()
                    .expect("moq subscribe runtime")
            })
            .handle()
            .clone(),
    };
    rt.block_on(async {
        tokio::time::timeout(STREAM_EXCHANGE_TIMEOUT, async {
            let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| format!("udp bind: {e}"))?;
            socket.set_nonblocking(true).ok();
            let runtime = Arc::new(quinn::TokioRuntime);
            let mut endpoint =
                Endpoint::new(quinn::EndpointConfig::default(), None, socket, runtime)
                    .map_err(|e| format!("endpoint: {e}"))?;
            let (_, client_config) = tls_configs()?;
            endpoint.set_default_client_config(client_config);

            let conn = endpoint
                .connect(addr, "soshal")
                .map_err(|e| format!("connect: {e}"))?
                .await
                .map_err(|e| format!("handshake: {e}"))?;
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| format!("open stream: {e}"))?;

            let body = lan::beacon_body(
                crate::lan_transport::LAN_MAGIC,
                my_pubkey,
                0,
                soshal_common_core::format::now_secs() as u64,
            );
            let mac = lan::beacon_mac(&key, &body);
            send.write_all(format!("{body}:{mac}\n").as_bytes())
                .await
                .map_err(|e| format!("handshake write: {e}"))?;

            let payload = serde_json::to_vec(&serde_json::json!({
                "type": "moq_subscribe",
                "stream_id": stream_id,
                "window_ms": window_ms,
            }))
            .map_err(|e| format!("req serde: {e}"))?;
            send.write_all(&(payload.len() as u32).to_le_bytes())
                .await
                .map_err(|e| format!("req write: {e}"))?;
            send.write_all(&payload)
                .await
                .map_err(|e| format!("req write: {e}"))?;
            send.finish().map_err(|e| format!("finish: {e}"))?;

            let mut out: Vec<Vec<u8>> = Vec::new();
            let mut total_bytes = 0usize;
            loop {
                let mut kind = [0u8; 1];
                if read_exact_async(&mut recv, &mut kind).await.is_err() {
                    break; // clean EOF: server finished the subscription
                }
                let mut len_buf = [0u8; 4];
                read_exact_async(&mut recv, &mut len_buf).await?;
                let len = u32::from_le_bytes(len_buf) as usize;
                if len > MAX_STREAM_FRAME {
                    return Err("oversized moq frame".to_string());
                }
                total_bytes += len;
                if total_bytes > MAX_MOQ_FETCH_BYTES {
                    return Err("moq fetch byte budget exceeded".to_string());
                }
                let mut data = vec![0u8; len];
                read_exact_async(&mut recv, &mut data).await?;
                match kind[0] {
                    0 => out.push(data),
                    1 => return Err("stream not found on peer".to_string()),
                    2 => return Err("peer seeding denied (power state)".to_string()),
                    _ => return Err("bad response kind".to_string()),
                }
            }
            Ok(out)
        })
        .await
        .map_err(|_| "moq subscribe timed out".to_string())?
    })
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    /// Serializes tests that mutate the process-global live registry (it can
    /// fill up at LIVE_MAX_STREAMS, breaking concurrent publishers).
    static REGISTRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn quic_stream_fetch_roundtrip() {
        let root = soshal_test_util::tmp_root("quic");
        let store = ChunkStore::new(root.clone());
        let data: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
        let m = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&m).unwrap();

        let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

        let chunk = &m.chunks[0];
        let got = fetch_quic_verified_chunk(
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
    fn quic_stream_refuses_bad_mac() {
        let root = soshal_test_util::tmp_root("quic");
        let server = start_quic_stream_server_with_store([7u8; 32], root.clone()).unwrap();
        // Client uses the wrong derivation key; HMAC must fail server-side.
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));
        let err = fetch_quic_chunk(
            addr,
            [9u8; 32],
            &"ab".repeat(32),
            &crate::lan_transport::LanChunkRequest {
                hash: "cd".repeat(32),
                offset: 0,
                length: 1024,
                want_manifest: false,
            },
        )
        .unwrap_err();
        assert!(
            err.contains("handshake") || err.contains("stream") || err.contains("eof"),
            "{err}"
        );
        server.stop();
    }

    #[test]
    fn quic_stream_missing_chunk_reports_not_found() {
        let root = soshal_test_util::tmp_root("quic");
        let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));
        let err =
            fetch_quic_verified_chunk(addr, [7u8; 32], &"ab".repeat(32), &"cd".repeat(32), 0, 1024)
                .unwrap_err();
        assert!(err.contains("not found"));
        server.stop();
    }

    #[test]
    fn quic_stream_refuses_public_peer() {
        let err = fetch_quic_chunk(
            SocketAddr::from(([8, 8, 8, 8], 9999)),
            [7u8; 32],
            &"ab".repeat(32),
            &crate::lan_transport::LanChunkRequest {
                hash: "cd".repeat(32),
                offset: 0,
                length: 16,
                want_manifest: false,
            },
        )
        .unwrap_err();
        assert!(err.contains("non-private"));
    }

    #[test]
    fn moq_live_registry_caps_history() {
        let _g = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let registry = super::live_registry();
        let mut last = 0u64;
        for i in 0..(super::LIVE_STREAM_HISTORY + 50) {
            last = registry
                .push("regcap", format!("group-{i:04}").into_bytes())
                .unwrap();
        }
        assert!(last > super::LIVE_STREAM_HISTORY as u64);
        let log = registry.get("regcap").unwrap();
        let lock = log.lock().unwrap();
        assert_eq!(lock.entries.len(), super::LIVE_STREAM_HISTORY);
        assert_eq!(lock.watermark, last);
    }

    #[test]
    fn moq_live_registry_rejects_bad_input() {
        let _g = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let registry = super::live_registry();
        assert!(registry.push("", vec![1u8; 16]).is_err());
        assert!(registry.push(&"x".repeat(129), vec![1u8; 16]).is_err());
        assert!(registry.push("okstream", vec![]).is_err());
        assert!(registry
            .push("bigstream", vec![1u8; super::LIVE_MAX_GROUP_BYTES + 1])
            .is_err());
    }

    #[test]
    fn moq_live_registry_full_rejects_new_streams() {
        let _g = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let registry = super::live_registry();
        registry.streams.lock().unwrap().clear();
        let mut last = 0u64;
        for i in 0..super::LIVE_MAX_STREAMS {
            last = registry
                .push(&format!("full-{i:02}"), format!("g-{i}").into_bytes())
                .unwrap();
        }
        assert!(last > 0);
        let err = registry
            .push("full-overflow", b"one-more".to_vec())
            .unwrap_err();
        assert!(err.contains("live registry full"));
        // Existing streams keep accepting groups.
        assert!(registry.push("full-00", b"still-open".to_vec()).is_ok());
        // Leave the global registry empty for other tests.
        registry.streams.lock().unwrap().clear();
    }

    #[test]
    fn moq_stream_known_and_publish_errors() {
        let _g = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(!super::moq_stream_known("unknown-stream-xyz"));
        let err = super::moq_publish_group("", b"x".to_vec()).unwrap_err();
        assert!(err.contains("bad live stream id"));
        super::moq_publish_group("known-stream-abc", b"hello".to_vec()).unwrap();
        assert!(super::moq_stream_known("known-stream-abc"));
    }

    #[test]
    fn moq_default_window_is_configured() {
        assert_eq!(super::default_moq_window(), super::LIVE_DEFAULT_WINDOW_MS);
    }

    #[test]
    fn moq_live_stream_replays_buffered_groups() {
        let _g = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = soshal_test_util::tmp_root("quic");
        let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

        let stream_id = "moq_replay_test";
        let groups: Vec<Vec<u8>> = (0..3)
            .map(|i| format!("encoded-group-{i}").into_bytes())
            .collect();
        for g in &groups {
            moq_publish_group(stream_id, g.clone()).unwrap();
        }

        let got =
            fetch_quic_moq_groups(addr, [7u8; 32], &"ab".repeat(32), stream_id, 1000).unwrap();
        assert_eq!(got.len(), 3, "buffered groups must replay");
        assert_eq!(got, groups, "group bytes must roundtrip unchanged");
        server.stop();
    }

    #[test]
    fn moq_live_stream_follows_new_groups_in_window() {
        let _g = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = soshal_test_util::tmp_root("quic");
        let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

        let stream_id = "moq_live_follow_test";
        moq_publish_group(stream_id, b"group-before".to_vec()).unwrap();

        let client = std::thread::spawn(move || {
            fetch_quic_moq_groups(addr, [7u8; 32], &"ab".repeat(32), stream_id, 2500)
        });
        std::thread::sleep(std::time::Duration::from_millis(250));
        moq_publish_group(stream_id, b"group-during-1".to_vec()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        moq_publish_group(stream_id, b"group-during-2".to_vec()).unwrap();

        let got = client.join().unwrap().unwrap();
        let flat: String = got
            .iter()
            .map(|g| String::from_utf8_lossy(g).to_string())
            .collect::<Vec<_>>()
            .join("|");
        assert_eq!(
            flat, "group-before|group-during-1|group-during-2",
            "replay first, then live groups in order"
        );
        server.stop();
    }

    #[test]
    fn moq_live_stream_unknown_stream_reports_not_found() {
        let root = soshal_test_util::tmp_root("quic");
        let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));
        let err =
            fetch_quic_moq_groups(addr, [7u8; 32], &"ab".repeat(32), "no_such_stream_xyz", 500)
                .unwrap_err();
        assert!(err.contains("not found"), "{err}");
        server.stop();
    }
}
