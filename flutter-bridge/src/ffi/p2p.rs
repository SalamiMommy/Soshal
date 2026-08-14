//! P2P FFI module
//!
//! Thin adapter over network-core's LAN stack: mDNS peer discovery
//! (advertise + browse), the HMAC-authenticated LAN chunk server, parallel
//! swarm downloads into mmap'd sparse files, and the thermal/battery-aware
//! seeding scheduler. All key material stays in the signer; this module only
//! receives derived LAN keys via `signer::lan_key()` — never across FFI.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_media_core::chunking::ChunkManifest;
use soshal_network_core::lan_transport::{start_lan_server, LanServerHandle};
use soshal_network_core::mdns::{MdnsAdvertiser, MdnsBrowser, MdnsPeer};
use soshal_network_core::power::{global_power_scheduler, PowerState};
use soshal_network_core::quic::QuicStreamServerHandle;
use soshal_network_core::swarm::{spawn_swarm_download, SwarmConfig, SwarmReport};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Mutex;

/// In-process P2P runtime: one advertiser, one browser, one LAN server, one
/// QUIC stream server, plus in-flight swarm download handles keyed by id.
struct P2pState {
    advertiser: Option<MdnsAdvertiser>,
    browser: Option<MdnsBrowser>,
    lan_server: Option<LanServerHandle>,
    quic_server: Option<QuicStreamServerHandle>,
    downloads: HashMap<String, std::thread::JoinHandle<SwarmReport>>,
    next_download: u64,
}

static P2P: Mutex<Option<P2pState>> = Mutex::new(None);

fn state() -> &'static Mutex<Option<P2pState>> {
    &P2P
}

fn state_mut() -> std::sync::MutexGuard<'static, Option<P2pState>> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

/// A peer discovered on the local subnet.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct P2pPeerDto {
    pub pubkey: String,
    pub ip: String,
    pub port: u16,
    pub quic_port: Option<u16>,
}

impl From<MdnsPeer> for P2pPeerDto {
    fn from(p: MdnsPeer) -> Self {
        Self {
            pubkey: p.pubkey,
            ip: p.addr.ip().to_string(),
            port: p.addr.port(),
            quic_port: p.quic_port,
        }
    }
}

/// Snapshot of the seeding scheduler state (what Dart pushes + what it reads).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct P2pPowerDto {
    pub mode: String,
    pub paused: bool,
    pub max_parallel_uploads: usize,
    pub upload_budget_bytes_per_sec: u64,
}

impl P2pPowerDto {
    fn from_state(state: PowerState) -> Self {
        let mode = state.seeding_mode();
        Self {
            mode: format!("{mode:?}").to_lowercase(),
            paused: mode.paused(),
            max_parallel_uploads: state.max_parallel_uploads(),
            upload_budget_bytes_per_sec: state.upload_budget_bytes_per_sec(),
        }
    }
}

/// Progress of an in-flight swarm download.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct P2pSwarmStatusDto {
    pub state: String,
    pub verified_chunks: usize,
    pub bytes_downloaded: u64,
    pub failures: usize,
    pub failed_hashes: Vec<String>,
}

impl P2pSwarmStatusDto {
    fn running() -> Self {
        Self {
            state: "running".to_string(),
            ..Self::default()
        }
    }

    fn from_report(r: SwarmReport) -> Self {
        Self {
            state: "done".to_string(),
            verified_chunks: r.verified_chunks,
            bytes_downloaded: r.bytes_downloaded,
            failures: r.failures,
            failed_hashes: r.failed_hashes,
        }
    }
}

/// Start advertising this device's chunk server over mDNS. `pubkey` may be
/// empty, in which case the unlocked signer's pubkey is used. `quic_port` is
/// advertised in TXT records if provided.
#[frb(sync, serialize)]
pub fn p2p_mdns_advertise_start(
    pubkey: String,
    port: u16,
    quic_port: Option<u16>,
) -> Result<bool, String> {
    let pubkey = if pubkey.is_empty() {
        crate::ffi::signer::signer_pubkey()?
    } else {
        pubkey
    };
    if pubkey.len() != 64 {
        return Err("invalid pubkey".to_string()).into();
    }
    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    if inner.advertiser.is_some() {
        return Ok(true).into();
    }
    inner.advertiser = Some(MdnsAdvertiser::start(&pubkey, "", port, quic_port)?);
    Ok(true).into()
}

/// Stop advertising (and drop the mDNS daemon).
#[frb(sync, serialize)]
pub fn p2p_mdns_advertise_stop() -> Result<bool, String> {
    if let Some(st) = state_mut().as_mut() {
        st.advertiser = None;
    }
    Ok(true).into()
}

/// Start browsing the local subnet for `_soshal._tcp` services (idempotent).
#[frb(sync, serialize)]
pub fn p2p_mdns_browse_start() -> Result<bool, String> {
    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    if inner.browser.is_none() {
        inner.browser = Some(MdnsBrowser::start()?);
    }
    Ok(true).into()
}

/// Drain pending mDNS discovery results as a JSON list of peers.
#[frb(sync, serialize)]
pub fn p2p_mdns_browse_drain() -> Result<Vec<P2pPeerDto>, String> {
    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    let Some(browser) = inner.browser.as_mut() else {
        return Ok(Vec::new()).into();
    };
    let peers = browser
        .drain_peers()
        .into_iter()
        .map(P2pPeerDto::from)
        .collect();
    Ok(peers).into()
}

/// Stop browsing (and drop the mDNS daemon).
#[frb(sync, serialize)]
pub fn p2p_mdns_browse_stop() -> Result<bool, String> {
    if let Some(st) = state_mut().as_mut() {
        st.browser = None;
    }
    Ok(true).into()
}

/// Start the HMAC-authenticated LAN chunk server on an ephemeral port.
/// The MAC key is derived from the unlocked signer — never crosses FFI.
/// `store_root` may be empty to serve the default chunk store.
#[frb(sync, serialize)]
pub fn p2p_lan_server_start(store_root: String) -> Result<u16, String> {
    let key = crate::ffi::signer::lan_key()?;
    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    if let Some(srv) = &inner.lan_server {
        return Ok(srv.port).into();
    }
    let handle = if store_root.is_empty() {
        start_lan_server(key)?
    } else {
        soshal_network_core::lan_transport::start_lan_server_with_store(
            key,
            PathBuf::from(store_root),
        )?
    };
    let port = handle.port;
    inner.lan_server = Some(handle);
    Ok(port).into()
}

/// Port of the running LAN chunk server, if any.
#[frb(sync, serialize)]
pub fn p2p_lan_server_port() -> Result<u16, String> {
    match state_mut().as_ref().and_then(|s| s.lan_server.as_ref()) {
        Some(h) => Ok(h.port).into(),
        None => Err("lan server not running".to_string()),
    }
}

/// Stop the LAN chunk server.
#[frb(sync, serialize)]
pub fn p2p_lan_server_stop() -> Result<bool, String> {
    if let Some(st) = state_mut().as_mut() {
        st.lan_server = None; // drop stops the listener thread
    }
    Ok(true).into()
}

/// Start the QUIC stream media server on an ephemeral port. Serves the same
/// HMAC-authenticated chunk CAS as the TCP LAN server, but over QUIC
/// bi-streams (connection migration across Wi-Fi/cellular, multiplexing).
/// `store_root` may be empty to serve the default chunk store.
#[frb(sync, serialize)]
pub fn p2p_quic_server_start(store_root: String) -> Result<u16, String> {
    let key = crate::ffi::signer::lan_key()?;
    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    if let Some(h) = inner.quic_server.as_ref() {
        return Ok(h.port).into();
    }
    let handle = if store_root.is_empty() {
        soshal_network_core::quic::start_quic_stream_server(key)?
    } else {
        soshal_network_core::quic::start_quic_stream_server_with_store(
            key,
            PathBuf::from(store_root),
        )?
    };
    let port = handle.port;
    inner.quic_server = Some(handle);
    Ok(port).into()
}

/// Port of the running QUIC stream server, if any.
#[frb(sync, serialize)]
pub fn p2p_quic_server_port() -> Result<u16, String> {
    match state_mut().as_ref().and_then(|s| s.quic_server.as_ref()) {
        Some(h) => Ok(h.port).into(),
        None => Err("quic server not running".to_string()),
    }
}

/// Stop the QUIC stream server.
#[frb(sync, serialize)]
pub fn p2p_quic_server_stop() -> Result<bool, String> {
    if let Some(st) = state_mut().as_mut() {
        if let Some(h) = st.quic_server.take() {
            h.stop();
        }
    }
    Ok(true).into()
}

/// Fetch one chunk range from a peer over a QUIC stream (blob-hash or
/// chunk-hash mode, same semantics as the TCP LAN fetch) and verify its
/// BLAKE3 hash before returning. `addr` must be a private IP.
#[frb(sync, serialize)]
pub fn p2p_quic_fetch_chunk(
    addr: String,
    hash: String,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, String> {
    let addr: SocketAddr = addr.parse().map_err(|e| format!("bad addr {addr}: {e}"))?;
    if !soshal_network_core::lan::is_private_ip(addr.ip()) {
        return Err(format!("refusing non-private peer {addr}")).into();
    }
    let key = crate::ffi::signer::lan_key()?;
    let my_pubkey = crate::ffi::signer::signer_pubkey()?;
    soshal_network_core::quic::fetch_quic_verified_chunk(
        addr, key, &my_pubkey, &hash, offset, length,
    )
}

/// Encode a MoQ group (JSON `MoqGroup`) into the on-stream binary framing.
#[frb(sync, serialize)]
pub fn p2p_moq_encode_group(group_json: String) -> Result<Vec<u8>, String> {
    let group: soshal_streaming_core::moq::MoqGroup =
        serde_json::from_str(&group_json).map_err(|e| format!("bad moq group: {e}"))?;
    soshal_streaming_core::moq::encode_group_stream(&group)
}

/// Decode a MoQ group from the on-stream binary framing back to JSON
/// `MoQGroup`. Bounds-checked against hostile input.
#[frb(sync, serialize)]
pub fn p2p_moq_decode_group(bytes: Vec<u8>) -> Result<String, String> {
    let group = soshal_streaming_core::moq::decode_group_stream(&bytes)?;
    serde_json::to_string(&group).map_err(|e| format!("moq serde: {e}"))
}

/// Publish one encoded MoQ group into the live registry under `stream_id`.
/// Bytes are opaque to the transport; the caller encodes with
/// `p2p_moq_encode_group`. Returns JSON `{"status","stream_id","groups"}`.
#[frb(sync, serialize)]
pub fn p2p_moq_publish_group(stream_id: String, encoded: Vec<u8>) -> Result<String, String> {
    if stream_id.is_empty() || stream_id.len() > 128 {
        return Err("bad live stream id".to_string()).into();
    }
    let seq = soshal_network_core::quic::moq_publish_group(&stream_id, encoded).map_err(|e| e)?;
    super::util::json_ok(serde_json::json!({
        "status": "published",
        "stream_id": stream_id,
        "groups": seq,
    }))
}

/// Subscribe to a live MoQ stream over QUIC for `window_ms`. Returns JSON
/// `{"groups":[<hex-encoded group frames>...]}` — one entry per group frame
/// sent by the peer (buffered replay, then live follow until idle/window end).
#[frb(sync, serialize)]
pub fn p2p_moq_subscribe_fetch(
    addr: String,
    stream_id: String,
    window_ms: u64,
) -> Result<String, String> {
    let addr: SocketAddr = addr.parse().map_err(|e| format!("bad addr {addr}: {e}"))?;
    if !soshal_network_core::lan::is_private_ip(addr.ip()) {
        return Err(format!("refusing non-private peer {addr}")).into();
    }
    let key = crate::ffi::signer::lan_key()?;
    let my_pubkey = crate::ffi::signer::signer_pubkey()?;
    let groups = soshal_network_core::quic::fetch_quic_moq_groups(
        addr, key, &my_pubkey, &stream_id, window_ms,
    )
    .map_err(|e| e)?;
    super::util::json_ok(serde_json::json!({
        "groups": groups.iter().map(hex::encode).collect::<Vec<_>>(),
    }))
}

/// Hash-only blob fetch from a single LAN peer: crawl the manifest over QUIC
/// (fallback TCP), then pull every chunk body (QUIC preferred, TCP fallback),
/// verify BLAKE3 per chunk + whole blob, absorb into the CAS, and write the
/// file. Returns JSON `{"success":true,"path","bytes"}`.
/// `ip` is a private-address string; `tcp_port` the LAN server port, and
/// `quic_port` the peer's QUIC stream port when advertised (else empty/None).
#[frb(sync, serialize)]
pub fn p2p_fetch_blob_from_peer(
    blob_hash: String,
    ip: String,
    tcp_port: u16,
    quic_port: Option<u16>,
    out_path: String,
) -> Result<String, String> {
    let ip: std::net::IpAddr = ip.parse().map_err(|e| format!("bad peer ip {ip}: {e}"))?;
    let peer = soshal_network_core::blob_grab::LanPeer {
        ip,
        tcp_port,
        quic_port,
    };
    let key = crate::ffi::signer::lan_key()?;
    let my_pubkey = crate::ffi::signer::signer_pubkey()?;
    let bytes = soshal_network_core::blob_grab::fetch_blob_from_peer(
        &peer, key, &my_pubkey, &blob_hash, &out_path,
    )
    .map_err(|e| e)?;
    super::util::json_ok(serde_json::json!({
        "success": true,
        "path": out_path,
        "bytes": bytes,
    }))
}

/// Kick off a swarm download of `manifest_json` from `peers_json` (a list of
/// `"ip:port"` strings) into `out_path`, then return the download id.
/// `quic_ports_json` is a parallel list of optional QUIC ports (null if not advertised).
/// Worker count is capped by the power scheduler: paused mode serializes
/// (1 worker) so background battery downloads can't ramp the radio.
#[frb(sync, serialize)]
pub fn p2p_swarm_download(
    manifest_json: String,
    peers_json: String,
    quic_ports_json: String,
    out_path: String,
    max_parallel: usize,
) -> Result<String, String> {
    let manifest: ChunkManifest =
        serde_json::from_str(&manifest_json).map_err(|e| format!("bad manifest: {e}"))?;
    if !manifest.is_valid() {
        return Err("manifest failed validation".to_string()).into();
    }
    let peers: Vec<String> =
        serde_json::from_str(&peers_json).map_err(|e| format!("bad peers: {e}"))?;
    let quic_ports: Vec<Option<u16>> =
        serde_json::from_str(&quic_ports_json).map_err(|e| format!("bad quic_ports: {e}"))?;
    let mut addrs: Vec<SocketAddr> = Vec::with_capacity(peers.len());
    for p in &peers {
        let addr: SocketAddr = p.parse().map_err(|e| format!("bad peer {p}: {e}"))?;
        if !soshal_network_core::lan::is_private_ip(addr.ip()) {
            return Err(format!("refusing non-private peer {p}")).into();
        }
        addrs.push(addr);
    }
    let key = crate::ffi::signer::lan_key()?;
    let my_pubkey = crate::ffi::signer::signer_pubkey()?;
    let scheduler_cap = global_power_scheduler().current().max_parallel_uploads();
    let capped = max_parallel.min(scheduler_cap.max(1));

    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    let id = format!("swarm-{}", inner.next_download);
    inner.next_download += 1;
    let handle = spawn_swarm_download(SwarmConfig {
        manifest,
        out_path: PathBuf::from(out_path),
        peers: addrs,
        quic_ports,
        key,
        my_pubkey,
        max_parallel: capped,
    });
    inner.downloads.insert(id.clone(), handle);
    Ok(id).into()
}

/// Poll a swarm download: `done` once the thread finished (report attached),
/// `running` otherwise. The finished handle is removed from the registry.
#[frb(sync, serialize)]
pub fn p2p_swarm_poll(id: String) -> Result<P2pSwarmStatusDto, String> {
    let mut st = state_mut();
    let inner = st.get_or_insert_with(P2pState::new);
    let Some(handle) = inner.downloads.remove(&id) else {
        return Err(format!("unknown download {id}")).into();
    };
    if !handle.is_finished() {
        inner.downloads.insert(id, handle);
        return Ok(P2pSwarmStatusDto::running()).into();
    }
    match handle.join() {
        Ok(report) => Ok(P2pSwarmStatusDto::from_report(report)).into(),
        Err(_) => Err(format!("download {id} panicked")).into(),
    }
}

/// Drop a swarm download handle (detaches the worker thread; the mmap file
/// keeps whatever chunks landed).
#[frb(sync, serialize)]
pub fn p2p_swarm_cancel(id: String) -> Result<bool, String> {
    if let Some(st) = state_mut().as_mut() {
        st.downloads.remove(&id);
    }
    Ok(true).into()
}

/// Push OS power/connectivity state into the seeding scheduler. Returns the
/// resulting mode snapshot. Call this whenever the OS battery or network
/// status changes (Dart polls battery_plus / connectivity_plus).
#[frb(sync, serialize)]
pub fn p2p_power_update(
    charging: bool,
    battery_percent: u8,
    cellular: bool,
    low_power_mode: bool,
) -> Result<P2pPowerDto, String> {
    let state = PowerState {
        charging,
        battery_percent: battery_percent.min(100),
        cellular,
        low_power_mode,
    };
    global_power_scheduler().update(state);
    Ok(P2pPowerDto::from_state(state)).into()
}

/// Current seeding mode snapshot.
#[frb(sync, serialize)]
pub fn p2p_power_mode() -> Result<P2pPowerDto, String> {
    Ok(P2pPowerDto::from_state(global_power_scheduler().current())).into()
}

/// Tear down all P2P runtime state (mDNS, LAN server, in-flight downloads).
#[frb(sync, serialize)]
pub fn p2p_stop_all() -> Result<bool, String> {
    if let Some(st) = state_mut().take() {
        drop(st); // advertiser/browser daemons + LAN listener stop on drop
    }
    Ok(true).into()
}

/// Encodes raw payload into rateless RaptorQ Fountain code packets.
#[frb(sync, serialize)]
pub fn p2p_encode_fountain_payload(data: Vec<u8>, redundancy_ratio: f32) -> Result<String, String> {
    let encoded = soshal_storage_core::erasure_fountain::encode_fountain(&data, redundancy_ratio)?;
    super::util::json_ok(encoded.manifest)
}

/// Decodes received Fountain code packets back into original payload.
#[frb(sync, serialize)]
pub fn p2p_decode_fountain_payload(
    manifest_json: String,
    packets_b64_json: String,
) -> Result<Vec<u8>, String> {
    let manifest: soshal_storage_core::erasure_fountain::FountainManifest =
        serde_json::from_str(&manifest_json).map_err(|e| format!("invalid manifest JSON: {e}"))?;
    let packets_b64: Vec<String> = serde_json::from_str(&packets_b64_json)
        .map_err(|e| format!("invalid packets JSON: {e}"))?;
    let packets: Vec<Vec<u8>> = packets_b64
        .into_iter()
        .filter_map(|b64| soshal_crypto_core::base64::base64_decode_bytes(&b64))
        .collect();
    soshal_storage_core::erasure_fountain::decode_fountain(&manifest, &packets)
}

impl P2pState {
    fn new() -> Self {
        Self {
            advertiser: None,
            browser: None,
            lan_server: None,
            quic_server: None,
            downloads: HashMap::new(),
            next_download: 0,
        }
    }
}
