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
//! peers (mesh-internal); inbound connections must present the HMAC beacon
//! line (registration datagram, re-sent per datagram since they are lossy)
//! before any micro-event is forwarded — same trust model as the LAN stream
//! handshake. Micro-events are presence signals — never authoritative.

use quinn::{ClientConfig, Connection, Endpoint, ServerConfig, TransportConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

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
const MAX_DATAGRAM_CONNS: usize = 64;
const REGISTRATION_KIND: &str = "__peer_key__";

/// SHA-256 fingerprint registry of peer TLS certificates seen over an HMAC
/// beacon exchange.
///
/// The registry is ADVISORY/diagnostic, not a TLS gate. QUIC reachability
/// cannot depend on it: a first-contact TLS handshake must succeed before the
/// HMAC beacon (which travels INSIDE the established connection) can ever be
/// exchanged, so requiring a pre-registered pin for every TLS handshake
/// creates a circular dependency that deadlocks every cross-device first
/// contact. Authorization is enforced by the HMAC beacon gate at the app
/// layer instead (see `spawn_conn_task` for datagrams and `auth_stream` for
/// the stream channel) — the same trust model as the TCP LAN transport, where
/// TLS does not gate access either. The registry is kept to record which
/// fingerprints have authenticated via HMAC (telemetry/audit only).
const MAX_TRUSTED_PEER_CERTS: usize = 1024;

struct TrustedCertRegistry {
    set: HashSet<[u8; 32]>,
    queue: std::collections::VecDeque<[u8; 32]>,
}

impl TrustedCertRegistry {
    fn new() -> Self {
        Self {
            set: HashSet::new(),
            queue: std::collections::VecDeque::new(),
        }
    }

    fn insert(&mut self, fp: [u8; 32]) {
        if self.set.contains(&fp) {
            return;
        }
        while self.queue.len() >= MAX_TRUSTED_PEER_CERTS {
            if let Some(old) = self.queue.pop_front() {
                self.set.remove(&old);
            }
        }
        self.set.insert(fp);
        self.queue.push_back(fp);
    }

    fn contains(&self, fp: &[u8; 32]) -> bool {
        self.set.contains(fp)
    }
}

static TRUSTED_PEER_CERTS: LazyLock<Mutex<TrustedCertRegistry>> =
    LazyLock::new(|| Mutex::new(TrustedCertRegistry::new()));

/// Compute the SHA-256 fingerprint of a DER-encoded certificate.
fn cert_fingerprint(cert: &CertificateDer<'_>) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(cert.as_ref());
    hasher.finalize().into()
}

/// Register a peer's TLS certificate fingerprint after a successful HMAC beacon
/// exchange. This is a diagnostic/audit record: the HMAC beacon gate at the app
/// layer is what actually authorizes the peer's traffic (see the note on
/// `TRUSTED_PEER_CERTS`). It is NOT consulted by TLS handshake acceptance —
/// first-contact handshakes must succeed before a beacon can ever arrive.
///
/// Called from the datagram auth path after `beacon_seq().check_and_record` succeeds.
pub(crate) fn register_trusted_cert(cert: &CertificateDer<'_>) {
    let fp = cert_fingerprint(cert);
    if let Ok(mut g) = TRUSTED_PEER_CERTS.lock() {
        g.insert(fp);
    }
}

/// Hybrid certificate verifier: validates certificates when possible,
/// but allows mesh-internal self-signed certs since identity is app-layer
/// (peer key datagram with HMAC beacon authentication).
#[derive(Debug)]
struct HybridCertVerifier;

impl ServerCertVerifier for HybridCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // Try system certificate validation first
        if let Ok(verified) =
            try_system_cert_validation(end_entity, intermediates, server_name, _ocsp_response, now)
        {
            return Ok(verified);
        }

        // Mesh fallback: accept self-signed certificates from first contact.
        // The `verify_tls13_signature`/`verify_tls12_signature` handlers below
        // still cryptographically bind the handshake to the presented cert's
        // public key, proving key possession — a passive injection MITM
        // remains defeated. Authorization (who may send/receive what) is
        // enforced by the HMAC beacon gate, which lives INSIDE the TLS
        // connection: the datagram channel drops every un-authed datagram
        // until a valid beacon arrives, and the stream channel refuses every
        // stream until the HMAC beacon line verifies. Requiring a
        // pre-registered cert pin for the handshake itself is impossible for
        // first contact (see `TRUSTED_PEER_CERTS` docs); the old registry
        // check deadlocked every cross-device first connection.
        let fp = cert_fingerprint(end_entity);
        let already_seen = TRUSTED_PEER_CERTS
            .lock()
            .map(|g| g.contains(&fp))
            .unwrap_or(false);

        if already_seen {
            log::debug!("QUIC: accepting mesh self-signed cert (registered via HMAC beacon)");
        } else {
            log::debug!(
                "QUIC: accepting first-contact mesh self-signed cert; HMAC beacon gate applies"
            );
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_signature(message, cert, dss, true)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_signature(message, cert, dss, false)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ED25519,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// Verify a handshake signature against the certificate's public key,
/// for both TLS 1.2 and TLS 1.3. Uses rustls's real signature verification
/// against the certificate's SubjectPublicKeyInfo — this works for mesh
/// self-signed certs too, proving the peer holds the private key matched to
/// the presented cert (defeats passive key-injection MITM).
fn verify_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
    tls12: bool,
) -> Result<HandshakeSignatureValid, rustls::Error> {
    // rustls validates the transcript signature against the certificate's
    // public key endpoint (webpki). Chain trust is separate (relaxed for the
    // mesh); this proves possession of the matching private key.
    let provider = rustls::crypto::ring::default_provider();
    let algos = provider.signature_verification_algorithms;
    if tls12 {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &algos)
    } else {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &algos)
    }
}

/// Server verifier anchored at the bundled Mozilla root store (webpki-roots).
/// Hosted/public endpoints presenting a real CA chain validate here. Mesh-
/// internal self-signed certs have no CA anchor and fail this check, falling
/// through to the mesh self-signed acceptance path in HybridCertVerifier.
static WEBPKI_VERIFIER: LazyLock<Arc<WebPkiServerVerifier>> = LazyLock::new(|| {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .expect("webpki server verifier build must succeed")
});

/// Attempt system certificate validation for known/public endpoints
fn try_system_cert_validation(
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
    server_name: &ServerName<'_>,
    ocsp_response: &[u8],
    now: UnixTime,
) -> Result<ServerCertVerified, rustls::Error> {
    // Chain validation against the bundled Mozilla roots: rejects expired or
    // wrong-host public certificates. Mesh self-signed certs (no CA chain)
    // fail here and fall through to the mesh acceptance path in
    // HybridCertVerifier::verify_server_cert.
    //
    // Pass the actual OCSP staple bytes so that public endpoints presenting a
    // revoked certificate alongside a valid OCSP staple are detected here.
    // (Mesh self-signed certs have no OCSP infrastructure; they are absent
    // from this code path because they fail the CA chain check first.)
    WEBPKI_VERIFIER.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
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
        self.stop.store(true, Ordering::Release);
    }
}

/// Spawns the QUIC datagram channel on its own thread + runtime.
/// `my_peer_key` is the hex pubkey advertised in registration datagrams;
/// `key` is the LAN key used to MAC them (HMAC beacon auth).
pub fn spawn_quic_datagram_channel(
    bind_port: u16,
    my_peer_key: String,
    key: [u8; 32],
) -> Result<QuicDatagramHandle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<Command>(1024);
    let (evt_tx, evt_rx) = std::sync::mpsc::sync_channel::<MicroEvent>(1024);
    // The runtime reports the real bound address (socket port 0 → ephemeral).
    let (bound_tx, bound_rx) = std::sync::mpsc::channel::<SocketAddr>();

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
                key,
                cmd_rx,
                evt_tx,
                thread_stop,
                bound_tx,
            ));
        })
        .map_err(|e| format!("quic thread: {e}"))?;

    let addr = bound_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], bind_port)));
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
    key: [u8; 32],
    mut cmd_rx: tokio::sync::mpsc::Receiver<Command>,
    evt_tx: std::sync::mpsc::SyncSender<MicroEvent>,
    stop: Arc<AtomicBool>,
    bound_tx: std::sync::mpsc::Sender<SocketAddr>,
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
    // Report the real bound address (bind_port 0 → ephemeral) so callers can
    // derive discoverable addresses without the runtime-renumber race.
    if let Some(b) = bound {
        let _ = bound_tx.send(b);
    }
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
    // Serialized HMAC registration blob per peer, reused across datagrams —
    // previously a fresh HMAC + JSON serialize ran on EVERY datagram send
    // (the beacon is only re-announced for loss recovery; peers re-verify it
    // the same way each time). Refreshed after the TTL so stale beacons are
    // never reused past the clock-skew window.
    let mut reg_cache: HashMap<SocketAddr, (std::time::Instant, Vec<u8>)> = HashMap::new();
    const REG_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);
    // Remote addrs that presented a valid HMAC beacon registration.
    let authed: Arc<Mutex<HashSet<SocketAddr>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut sweep = tokio::time::interval(std::time::Duration::from_secs(30));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        tokio::select! {
            _ = sweep.tick() => {
                // Evict dead connections (closed by peer or idle timeout) so
                // the map cannot grow unboundedly over long sessions.
                conns.retain(|_, c| c.close_reason().is_none());
                reg_cache.retain(|addr, (at, _)| {
                    conns.contains_key(addr) && at.elapsed() < REG_CACHE_TTL
                });
                if let Ok(mut a) = authed.lock() {
                    a.retain(|addr| conns.contains_key(addr));
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { continue; };
                // Privacy invariant: only private hosts may connect.
                if !crate::lan::is_private_ip(incoming.remote_address().ip()) {
                    continue;
                }
                if conns.len() >= MAX_DATAGRAM_CONNS {
                    continue;
                }
                if let Ok(conn) = incoming.await {
                    conns.insert(conn.remote_address(), conn.clone());
                    spawn_conn_task(conn, evt_tx.clone(), bound, key, authed.clone());
                }
            }
            cmd = recv_cmd(&mut cmd_rx) => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    Command::Send { peer, event } => {
                        if !crate::lan::is_private_ip(peer.ip()) {
                            eprintln!("quic datagram SSRF blocked: {peer}");
                            continue;
                        }
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
                                        spawn_conn_task(c.clone(), evt_tx.clone(), bound, key, authed.clone());
                                        c
                                    }
                                    Err(_) => continue,
                                }
                            }
                        };
                        // register identity: HMAC beacon line, re-sent with
                        // every datagram (datagrams are lossy — a dropped
                        // registration must not deadlock the channel). The
                        // serialized registration is cached per peer; only the
                        // first send per TTL window pays the HMAC cost.
                        let reg_bytes = match reg_cache.get(&peer) {
                            Some((at, b)) if at.elapsed() < REG_CACHE_TTL => b.clone(),
                            _ => {
                                let body = crate::lan::beacon_body(
                                    crate::lan_transport::LAN_MAGIC,
                                    &my_peer_key,
                                    0,
                                    soshal_common_core::format::now_secs() as u64,
                                    &hex::encode(crate::lan::fresh_nonce()),
                                );
                                let mac = crate::lan::beacon_mac(&key, &body);
                                let reg = MicroEvent {
                                    kind: REGISTRATION_KIND.to_string(),
                                    payload: format!("{body}:{mac}"),
                                };
                                let bytes = serde_json::to_vec(&reg).unwrap_or_default();
                                reg_cache.insert(peer, (std::time::Instant::now(), bytes.clone()));
                                bytes
                            }
                        };
                        if reg_bytes.len() <= MAX_DATAGRAM {
                            let _ = conn.send_datagram(reg_bytes.into());
                        }
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
    key: [u8; 32],
    authed: Arc<Mutex<HashSet<SocketAddr>>>,
) {
    tokio::spawn(async move {
        loop {
            match conn.read_datagram().await {
                Ok(bytes) => {
                    let remote = conn.remote_address();
                    let is_authed = authed.lock().map(|g| g.contains(&remote)).unwrap_or(false);
                    let Ok(ev) = serde_json::from_slice::<MicroEvent>(&bytes) else {
                        continue;
                    };
                    if !is_authed {
                        // First datagram must be the HMAC beacon registration;
                        // anything else is ignored until the beacon verifies.
                        if ev.kind == REGISTRATION_KIND {
                            if let Some((pk, _, nonce)) = crate::lan::parse_beacon(
                                &key,
                                crate::lan_transport::LAN_MAGIC,
                                ev.payload.trim(),
                                0,
                                soshal_common_core::format::now_secs() as u64,
                            ) {
                                // Replay guard: a captured beacon line has a
                                // duplicate nonce and is rejected.
                                if crate::lan::beacon_seq().check_and_record(&pk, &nonce) {
                                    if let Ok(mut g) = authed.lock() {
                                        g.insert(remote);
                                    }
                                    // Record the peer's TLS cert fingerprint in
                                    // the advisory registry (HMAC-authenticated
                                    // peer audit). Diagnostic only — TLS
                                    // acceptance is not gated on it.
                                    // `peer_certificate()` is Some for verified
                                    // QUIC connections; skip if unavailable.
                                    if let Some(cert) = conn.peer_identity()
                                        .and_then(|id| id.downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>().ok())
                                        .and_then(|certs| certs.into_iter().next())
                                    {
                                        register_trusted_cert(&cert);
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    if ev.kind != REGISTRATION_KIND {
                        let _ = evt_tx.try_send(ev);
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
    cfg.max_concurrent_bidi_streams(quinn::VarInt::from_u32(16));
    cfg.max_concurrent_uni_streams(quinn::VarInt::from_u32(16));
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
        .set_certificate_verifier(Arc::new(HybridCertVerifier));

    let client_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(client_tls)
        .map_err(|e| format!("quic client config: {e}"))?;
    let mut client_config = ClientConfig::new(Arc::new(client_crypto));
    client_config.transport_config(Arc::new(transport_config()));

    let pair = (
        ServerConfig::with_crypto(Arc::new(server_crypto)),
        client_config,
    );
    let _ = CFG.set(pair.clone());
    // Record the locally generated cert in the advisory registry (diagnostic
    // only; TLS accepts first-contact self-signed certs regardless, gated at
    // the app layer by the HMAC beacon).
    register_trusted_cert(&cert_der);
    Ok(pair)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Picks a free UDP port using a bind-probe socket. Small TOCTOU window
    /// (matched by the host transport tests), acceptable in tests.
    fn free_port() -> u16 {
        let s = std::net::UdpSocket::bind(("0.0.0.0", 0)).unwrap();
        let port = s.local_addr().unwrap().port();
        drop(s);
        port
    }

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

    /// Regression (2026-09-20, HIGH #1): QUIC cross-device first contact used
    /// to deadlock. `HybridCertVerifier` required the peer's self-signed cert
    /// to already be in the HMAC-pinned registry, but a peer's fingerprint can
    /// only be registered AFTER a successful HMAC beacon exchange — which
    /// requires a live TLS connection — so no first TLS handshake could ever
    /// succeed between two devices. The verifier now accepts first-contact
    /// self-signed certs (signature validation still proves key possession);
    /// authorization is enforced by the HMAC beacon gate at the app layer.
    #[test]
    fn hybrid_verifier_accepts_unpinned_first_contact_cert() {
        // Fresh cert NOT in the process registry: simulates a brand-new peer.
        let cert = rcgen::generate_simple_self_signed(vec!["soshal.local".to_string()]).unwrap();
        let end_entity = CertificateDer::from(cert.cert.der().to_vec());
        let name = rustls::pki_types::ServerName::try_from("soshal.local").unwrap();
        let now = rustls::pki_types::UnixTime::now();

        let result = HybridCertVerifier.verify_server_cert(&end_entity, &[], &name, &[], now);
        assert!(
            result.is_ok(),
            "first-contact self-signed cert must be accepted (HMAC gate enforces auth): {result:?}"
        );
    }

    /// Cross-device-shaped first contact over the datagram channel: peer A
    /// sends to peer B that it has never "met" before, with only the shared
    /// LAN key. Before the fix this failed the TLS handshake (cert not pinned);
    /// now the HMAC beacon inside completes and the micro-event is delivered.
    #[test]
    fn quic_datagram_first_contact_delivers() {
        let port_a = free_port();
        let port_b = free_port();
        let a = spawn_quic_datagram_channel(port_a, "a".repeat(64), [7u8; 32]).unwrap();
        let b = spawn_quic_datagram_channel(port_b, "b".repeat(64), [7u8; 32]).unwrap();
        let b_addr = SocketAddr::from(([127, 0, 0, 1], port_b));
        let ev = MicroEvent {
            kind: "typing".to_string(),
            payload: r#"{"to":"b"}"#.to_string(),
        };
        a.send_to(b_addr, ev.clone()).unwrap();

        // Poll for the delivered event (datagrams are lossy; bounded wait).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(got) = b.try_recv() {
                assert_eq!(got, ev);
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("micro-event never delivered on first contact");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        a.stop();
        b.stop();
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
use crate::lan_transport::ResponseKind;
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
struct LiveStreamLog {
    entries: std::collections::BTreeMap<u64, Arc<Vec<u8>>>,
    watermark: u64,
    last_activity: std::time::Instant,
}

impl Default for LiveStreamLog {
    fn default() -> Self {
        Self {
            entries: std::collections::BTreeMap::new(),
            watermark: 0,
            last_activity: std::time::Instant::now(),
        }
    }
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

    /// Remove a stream from the registry (called when a broadcast ends),
    /// releasing its buffered groups. No-op if absent.
    fn remove(&self, stream_id: &str) -> bool {
        self.streams
            .lock()
            .map(|mut s| s.remove(stream_id).is_some())
            .unwrap_or(false)
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
        // Opportunistic idle-eviction so a completed broadcast's groups are
        // released instead of lingering; runs from any publisher. Bounded by
        // LIVE_MAX_STREAMS, so this sweep is cheap.
        let now = std::time::Instant::now();
        let idle_cutoff = now.checked_sub(StdDuration::from_secs(10)).unwrap_or(now);
        let stale: Vec<String> = streams
            .iter()
            .filter(|(id, log)| {
                *id != stream_id
                    && log
                        .lock()
                        .map(|l| l.last_activity < idle_cutoff)
                        .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            streams.remove(&id);
        }
        if streams.len() >= LIVE_MAX_STREAMS && !streams.contains_key(stream_id) {
            return Err("live registry full".to_string());
        }
        let log = streams.entry(stream_id.to_string()).or_default();
        let mut lock = log
            .lock()
            .map_err(|_| "live stream log poisoned".to_string())?;
        lock.watermark += 1;
        let seq = lock.watermark;
        lock.last_activity = std::time::Instant::now();
        lock.entries.insert(seq, Arc::new(encoded));
        let mut total_bytes: usize = lock.entries.values().map(|g| g.len()).sum();
        while lock.entries.len() > LIVE_STREAM_HISTORY
            || (total_bytes > LIVE_MAX_REPLAY_BYTES && lock.entries.len() > 1)
        {
            if let Some((_, g)) = lock.entries.pop_first() {
                total_bytes -= g.len();
            }
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

/// Remove a live stream from the registry (broadcast ended).
pub fn moq_stream_remove(stream_id: &str) -> bool {
    live_registry().remove(stream_id)
}
const STREAM_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const STREAM_EXCHANGE_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Cap on concurrent QUIC stream handshakes still waiting for the HMAC
/// beacon line (see D2 hardening: slow-loris over QUIC).
const MAX_PENDING_AUTH_STREAMS: usize = 8;
const AUTH_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// Handle for the QUIC stream media server (mirrors `LanServerHandle`).
pub struct QuicStreamServerHandle {
    pub port: u16,
    stop: Arc<AtomicBool>,
    stop_notify: Arc<tokio::sync::Notify>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl QuicStreamServerHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.stop_notify.notify_one();
    }
}

impl Drop for QuicStreamServerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.stop_notify.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
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
    let stop_notify = Arc::new(tokio::sync::Notify::new());
    let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
    let thread_stop = stop.clone();
    let thread_notify = stop_notify.clone();
    let thread = std::thread::Builder::new()
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
            rt.block_on(run_stream_server(
                key,
                store_root,
                port_tx,
                thread_stop,
                thread_notify,
            ));
        })
        .map_err(|e| format!("quic stream thread: {e}"))?;
    let port = port_rx
        .recv_timeout(STREAM_CONNECT_TIMEOUT)
        .map_err(|e| format!("quic stream bind: {e}"))?;
    Ok(QuicStreamServerHandle {
        port,
        stop,
        stop_notify,
        thread: Some(thread),
    })
}

async fn run_stream_server(
    key: [u8; 32],
    store_root: std::path::PathBuf,
    port_tx: std::sync::mpsc::Sender<u16>,
    stop: Arc<AtomicBool>,
    stop_notify: Arc<tokio::sync::Notify>,
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
    const MAX_STREAM_CONNS: usize = 32;
    let active_conns = Arc::new(AtomicUsize::new(0));
    // Unauthenticated handshakes are separately capped: a private-IP flood
    // of idle connections must not occupy every connection slot for the
    // whole AUTH_TIMEOUT window (slow-loris over QUIC).
    let pending_auth = Arc::new(AtomicUsize::new(0));
    loop {
        tokio::select! {
            _ = stop_notify.notified() => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break; };
                // Privacy invariant (mirrors TCP LAN transport): only private
                // hosts may connect; refuse the rest before handshake.
                if !crate::lan::is_private_ip(incoming.remote_address().ip()) {
                    continue;
                }
                if active_conns.load(Ordering::SeqCst) >= MAX_STREAM_CONNS {
                    continue;
                }
                if let Ok(conn) = incoming.await {
                    active_conns.fetch_add(1, Ordering::SeqCst);
                    let counter = active_conns.clone();
                    let pa = pending_auth.clone();
                    let store = store.clone();
                    tokio::spawn(async move {
                        serve_stream_conn(conn, key, store, pa).await;
                        counter.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            }
        }
    }
    // Gracefully close so connections drain promptly even when a long-lived
    // client (e.g. the process-global QUIC chunk pool) still holds a
    // connection. The join in Drop must never block forever: a peer that
    // never acknowledges its close would otherwise strand the thread.
    endpoint.close(quinn::VarInt::from_u32(0), b"soshal shutdown");
    drop(endpoint);
}

/// Accepts bi-streams on one connection and serves each in its own task so
/// parallel chunk fetches never head-of-line block each other.
async fn serve_stream_conn(
    conn: Connection,
    key: [u8; 32],
    store: ChunkStore,
    pending_auth: Arc<AtomicUsize>,
) {
    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                let store = store.clone();
                let pa = pending_auth.clone();
                tokio::spawn(async move { serve_stream(send, recv, key, store, pa).await });
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
    pending_auth: Arc<AtomicUsize>,
) {
    // Auth: exactly like the TCP transport, the client must open with the
    // HMAC'd beacon line; anything else gets a refused stream. Bytes past
    // the newline (coalesced request frame) come back for the frame reads.
    if pending_auth.load(Ordering::SeqCst) >= MAX_PENDING_AUTH_STREAMS {
        let _ = send.write_all(b"ERR\n").await;
        let _ = send.finish();
        return;
    }
    pending_auth.fetch_add(1, Ordering::SeqCst);
    let auth = auth_stream(&mut recv, &key).await;
    pending_auth.fetch_sub(1, Ordering::SeqCst);
    let mut leftover = match auth {
        Ok(l) if !global_power_scheduler().mode().paused() => l,
        _ => {
            let _ = send.write_all(b"ERR\n").await;
            let _ = send.finish();
            return;
        }
    };

    // Request frame: [u32 len][JSON LanChunkRequest], same as TCP.
    let mut len_buf = [0u8; 4];
    if !matches!(
        tokio::time::timeout(
            STREAM_EXCHANGE_TIMEOUT,
            read_exact_with_leftover(&mut recv, &mut leftover, &mut len_buf)
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let req_len = u32::from_le_bytes(len_buf) as usize;
    if req_len == 0 || req_len > MAX_STREAM_FRAME {
        let _ = write_stream_frame(&mut send, ResponseKind::BadRequest, &[]).await;
        let _ = send.finish();
        return;
    }
    let mut req_bytes = vec![0u8; req_len];
    if !matches!(
        tokio::time::timeout(
            STREAM_EXCHANGE_TIMEOUT,
            read_exact_with_leftover(&mut recv, &mut leftover, &mut req_bytes)
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let req: StreamRequest = match serde_json::from_slice(&req_bytes) {
        Ok(r) => r,
        Err(_) => {
            let _ = write_stream_frame(&mut send, ResponseKind::BadRequest, &[]).await;
            let _ = send.finish();
            return;
        }
    };

    match req {
        StreamRequest::Chunk(chunk_req) => {
            if chunk_req.want_manifest {
                let kind = if chunk_req.hash.len() != 64 {
                    ResponseKind::BadRequest
                } else if global_power_scheduler().mode().paused() {
                    ResponseKind::Denied
                } else {
                    match store.load_manifest(&chunk_req.hash) {
                        Some(m) => match serde_json::to_vec(&m) {
                            Ok(bytes) if bytes.len() <= MAX_STREAM_FRAME => {
                                if let Err(e) =
                                    write_stream_frame(&mut send, ResponseKind::Ok, &bytes).await
                                {
                                    log::warn!("quic response write: {e}");
                                    return;
                                }
                                if let Err(e) = send.finish() {
                                    log::warn!("quic response write: {e}");
                                    let _ = send.write_all(&[3u8]).await;
                                    return;
                                }
                                return;
                            }
                            Ok(_) | Err(_) => ResponseKind::BadRequest,
                        },
                        None => ResponseKind::NotFound,
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
                ResponseKind::BadRequest
            } else if global_power_scheduler().mode().paused() {
                ResponseKind::Denied
            } else {
                let manifest = store
                    .load_manifest(&chunk_req.hash)
                    .or_else(|| store.find_manifest_containing_chunk(&chunk_req.hash));
                match manifest
                    .as_ref()
                    .and_then(|m| store.blob_slice(m, chunk_req.offset, chunk_req.length))
                {
                    Some(bytes) => {
                        if let Err(e) =
                            write_stream_frame(&mut send, ResponseKind::Ok, &bytes).await
                        {
                            log::warn!("quic response write: {e}");
                            return;
                        }
                        if let Err(e) = send.finish() {
                            log::warn!("quic response write: {e}");
                            let _ = send.write_all(&[3u8]).await;
                            return;
                        }
                        return;
                    }
                    None => ResponseKind::NotFound,
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
                let _ = write_stream_frame(&mut send, ResponseKind::BadRequest, &[]).await;
                let _ = send.finish();
            } else if global_power_scheduler().mode().paused() {
                let _ = write_stream_frame(&mut send, ResponseKind::Denied, &[]).await;
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
        let _ = write_stream_frame(send, ResponseKind::NotFound, &[]).await;
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
        let mut last_seq = 0u64;
        let mut replayed_bytes = 0usize;
        for (seq, g) in lock.entries.iter() {
            if replayed_bytes + g.len() > LIVE_MAX_REPLAY_BYTES {
                break;
            }
            replayed_bytes += g.len();
            last_seq = *seq;
            replay.push(g.clone());
        }
        let wm = if replay.is_empty() {
            lock.watermark
        } else {
            last_seq
        };
        (replay, wm)
    };
    let mut watermark = wm;
    for group in &replay {
        if write_stream_frame(send, ResponseKind::Ok, group.as_slice())
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
            if write_stream_frame(send, ResponseKind::Ok, group.as_slice())
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

async fn auth_stream(recv: &mut quinn::RecvStream, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let mut line = Vec::with_capacity(128);
    let mut leftover = Vec::new();
    let mut buf = [0u8; 128];
    let mut total_bytes = 0usize;
    const MAX_AUTH_BYTES: usize = 4096;
    tokio::time::timeout(AUTH_TIMEOUT, async {
        loop {
            match recv.read(&mut buf).await {
                Ok(None) => return Err("eof during handshake".to_string()),
                Ok(Some(n)) => {
                    if let Some(pos) = buf[..n].iter().position(|&b| b == b'\n') {
                        line.extend_from_slice(&buf[..pos]);
                        leftover.extend_from_slice(&buf[pos + 1..n]);
                        break;
                    }
                    line.extend_from_slice(&buf[..n]);
                    total_bytes += n;
                    if total_bytes > MAX_AUTH_BYTES {
                        return Err("auth too large".into());
                    }
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
            Some((pk, _, nonce)) if crate::lan::beacon_seq().check_and_record(&pk, &nonce) => {
                Ok(())
            }
            Some(_) | None => Err("bad hmac beacon".to_string()),
        }
    })
    .await
    .map_err(|_| "auth handshake timed out".to_string())??;
    Ok(leftover)
}

/// `read_exact_async`, but first drains bytes already read (coalesced with
/// the auth beacon) before touching the stream.
async fn read_exact_with_leftover(
    recv: &mut quinn::RecvStream,
    leftover: &mut Vec<u8>,
    buf: &mut [u8],
) -> Result<(), String> {
    let take = leftover.len().min(buf.len());
    buf[..take].copy_from_slice(&leftover[..take]);
    leftover.drain(..take);
    read_exact_async(recv, &mut buf[take..]).await
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

async fn write_stream_frame(
    send: &mut quinn::SendStream,
    kind: ResponseKind,
    payload: &[u8],
) -> Result<(), String> {
    let len_bytes = (payload.len() as u32).to_le_bytes();
    let header = [
        kind as u8,
        len_bytes[0],
        len_bytes[1],
        len_bytes[2],
        len_bytes[3],
    ];
    send.write_all(&header)
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

    let nonce = hex::encode(lan::fresh_nonce());
    let body = lan::beacon_body(
        crate::lan_transport::LAN_MAGIC,
        my_pubkey,
        0,
        soshal_common_core::format::now_secs() as u64,
        &nonce,
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

/// Run a `Future` to completion from a sync context. When inside an existing
/// Tokio runtime the future is **spawned** onto it (avoiding the
/// "Cannot block the current thread" panic); otherwise it blocks on a
/// dedicated static runtime.
fn run_async_blocking<F, T>(fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
    T: Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            h.spawn(async move {
                let result = fut.await;
                let _ = tx.send(result);
            });
            rx.blocking_recv()
                .map_err(|_| "run_async_blocking: sender dropped".to_string())?
        }
        Err(_) => {
            static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> =
                std::sync::OnceLock::new();
            SHARED_RT
                .get_or_init(|| {
                    tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .worker_threads(2)
                        .build()
                        .expect("quic blocking runtime")
                })
                .block_on(fut)
        }
    }
}

/// Reusable QUIC client for bulk chunk fetches: one endpoint and one
/// connection per peer, reused across chunks — the swarm download path avoids
/// a fresh socket + full TLS handshake per chunk.
pub struct QuicChunkPool {
    endpoint: Endpoint,
    conns: std::sync::Mutex<
        std::collections::HashMap<SocketAddr, (quinn::Connection, std::time::Instant)>,
    >,
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
            .and_then(|guard| guard.get(&addr).map(|(c, _)| c.clone()))
    }

    fn store(&self, addr: SocketAddr, conn: &quinn::Connection) {
        if let Ok(mut guard) = self.conns.lock() {
            guard.insert(addr, (conn.clone(), std::time::Instant::now()));
        }
    }

    fn touch(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.conns.lock() {
            if let Some(entry) = guard.get_mut(&addr) {
                entry.1 = std::time::Instant::now();
            }
        }
    }

    fn evict(&self, addr: SocketAddr) {
        if let Ok(mut guard) = self.conns.lock() {
            guard.remove(&addr);
        }
    }

    /// Close and drop connections idle for longer than `idle`, so a pool that
    /// outlives a swarm download (e.g. cached app-wide) does not hold open
    /// sockets to transient peers forever. Called opportunistically on fetch.
    fn evict_idle(&self, idle: std::time::Duration) {
        if let Ok(mut guard) = self.conns.lock() {
            let now = std::time::Instant::now();
            let stale: Vec<SocketAddr> = guard
                .iter()
                .filter(|(_, (_, last))| now.saturating_duration_since(*last) > idle)
                .map(|(a, _)| *a)
                .collect();
            for a in stale {
                guard.remove(&a);
            }
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
        // Opportunistic idle eviction (keep pool from retaining stale peers).
        self.evict_idle(std::time::Duration::from_secs(60));
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
            self.touch(addr);
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

static SHARED_QUIC_POOL: LazyLock<std::sync::Mutex<Option<Arc<QuicChunkPool>>>> =
    LazyLock::new(|| std::sync::Mutex::new(QuicChunkPool::new().ok().map(Arc::new)));

pub fn shared_quic_pool() -> Option<Arc<QuicChunkPool>> {
    let mut guard = SHARED_QUIC_POOL.lock().ok()?;
    if guard.is_none() {
        *guard = QuicChunkPool::new().ok().map(Arc::new);
    }
    guard.clone()
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
    let my_pubkey = my_pubkey.to_owned();
    let req = req.clone();
    run_async_blocking(async move {
        tokio::time::timeout(STREAM_EXCHANGE_TIMEOUT, async {
            if let Some(pool) = shared_quic_pool() {
                if let Ok(data) = pool.fetch_chunk(addr, key, &my_pubkey, &req).await {
                    return Ok(data);
                }
            }
            let (_endpoint, conn) = quic_connect(addr).await?;
            exchange_chunk(&conn, key, &my_pubkey, &req).await
        })
        .await
        .map_err(|_| "quic exchange timed out".to_string())?
    })
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
    let my_pubkey = my_pubkey.to_owned();
    let req = req.clone();
    run_async_blocking(async move {
        tokio::time::timeout(STREAM_EXCHANGE_TIMEOUT, async {
            let (_endpoint, conn) = quic_connect(addr).await?;
            exchange_chunk(&conn, key, &my_pubkey, &req).await
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
    let my_pubkey = my_pubkey.to_owned();
    let stream_id = stream_id.to_owned();
    run_async_blocking(async move {
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
                &my_pubkey,
                0,
                soshal_common_core::format::now_secs() as u64,
                &hex::encode(lan::fresh_nonce()),
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
                if len > LIVE_MAX_GROUP_BYTES {
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
        let server = start_quic_stream_server_with_store([7u8; 32], root).unwrap();
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

    #[test]
    fn test_trusted_cert_registry_bounded_eviction() {
        let mut registry = TrustedCertRegistry::new();
        assert_eq!(registry.set.len(), 0);

        // Fill up to capacity
        for i in 0..MAX_TRUSTED_PEER_CERTS {
            let mut fp = [0u8; 32];
            fp[..usize::min(8, 32)].copy_from_slice(&(i as u64).to_le_bytes());
            registry.insert(fp);
        }
        assert_eq!(registry.set.len(), MAX_TRUSTED_PEER_CERTS);
        assert_eq!(registry.queue.len(), MAX_TRUSTED_PEER_CERTS);

        // Oldest entry (index 0) must be present
        let mut fp0 = [0u8; 32];
        fp0[..8].copy_from_slice(&0u64.to_le_bytes());
        assert!(registry.contains(&fp0));

        // Insert one more entry beyond capacity
        let mut fp_new = [0u8; 32];
        fp_new[..8].copy_from_slice(&(MAX_TRUSTED_PEER_CERTS as u64).to_le_bytes());
        registry.insert(fp_new);

        // Size must remain bounded
        assert_eq!(registry.set.len(), MAX_TRUSTED_PEER_CERTS);
        assert_eq!(registry.queue.len(), MAX_TRUSTED_PEER_CERTS);

        // Oldest entry 0 must have been evicted
        assert!(!registry.contains(&fp0));
        // New entry must be present
        assert!(registry.contains(&fp_new));
        // Entry 1 must still be present
        let mut fp1 = [0u8; 32];
        fp1[..8].copy_from_slice(&1u64.to_le_bytes());
        assert!(registry.contains(&fp1));
    }
}
