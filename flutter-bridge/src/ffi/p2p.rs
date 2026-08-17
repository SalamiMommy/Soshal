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
#[frb(serialize)]
pub async fn p2p_quic_fetch_chunk(
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
#[frb(serialize)]
pub async fn p2p_moq_subscribe_fetch(
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
#[frb(serialize)]
pub async fn p2p_fetch_blob_from_peer(
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

#[cfg(test)]
mod tests {
    use super::*;

    static GLOBAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn sample_group_json() -> String {
        serde_json::json!({
            "group_sequence": 7,
            "objects": [
                {
                    "header": {
                        "track_id": 1,
                        "group_sequence": 7,
                        "object_sequence": 0,
                        "payload_size": 3,
                        "track_type": "VideoKeyframe",
                        "timestamp_ms": 1000
                    },
                    "payload": [0, 1, 2]
                },
                {
                    "header": {
                        "track_id": 2,
                        "group_sequence": 7,
                        "object_sequence": 1,
                        "payload_size": 2,
                        "track_type": "AudioDatagram",
                        "timestamp_ms": 1050
                    },
                    "payload": [255, 254]
                }
            ]
        })
        .to_string()
    }

    fn valid_manifest_json() -> String {
        serde_json::json!({
            "blob_hash": "a".repeat(64),
            "total_size": 0,
            "chunks": []
        })
        .to_string()
    }

    #[test]
    fn test_moq_group_roundtrip() {
        let wire = super::p2p_moq_encode_group(sample_group_json()).unwrap();
        assert!(!wire.is_empty());
        let json = super::p2p_moq_decode_group(wire).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["group_sequence"], 7);
        assert_eq!(v["objects"].as_array().unwrap().len(), 2);
        assert_eq!(v["objects"][0]["payload"], serde_json::json!([0, 1, 2]));
        assert_eq!(v["objects"][1]["payload"], serde_json::json!([255, 254]));
        assert_eq!(v["objects"][0]["header"]["track_type"], "VideoKeyframe");
        assert_eq!(v["objects"][1]["header"]["track_type"], "AudioDatagram");
    }

    #[test]
    fn test_moq_encode_rejects_bad_json() {
        let e = super::p2p_moq_encode_group("not json".to_string()).unwrap_err();
        assert!(e.contains("bad moq group"), "got {e}");
    }

    #[test]
    fn test_moq_decode_rejects_hostile_input() {
        assert!(super::p2p_moq_decode_group(Vec::new()).is_err());
        assert!(super::p2p_moq_decode_group(vec![0x01, 0x00, 0x00, 0x00]).is_err());
        let mut wire = super::p2p_moq_encode_group(sample_group_json()).unwrap();
        wire.push(0xFF);
        let e = super::p2p_moq_decode_group(wire).unwrap_err();
        assert!(e.contains("trailing"), "got {e}");
        let mut bad = Vec::new();
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.push(3u8);
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.push(0xAA);
        let e = super::p2p_moq_decode_group(bad).unwrap_err();
        assert!(e.contains("bad track type"), "got {e}");
    }

    #[test]
    fn test_moq_publish_validation_and_registry() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let e = super::p2p_moq_publish_group(String::new(), vec![1]).unwrap_err();
        assert!(e.contains("bad live stream id"), "got {e}");
        let e = super::p2p_moq_publish_group("x".repeat(129), vec![1]).unwrap_err();
        assert!(e.contains("bad live stream id"), "got {e}");
        let encoded = super::p2p_moq_encode_group(sample_group_json()).unwrap();
        let first =
            super::p2p_moq_publish_group("ffi-test-stream".to_string(), encoded.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(v["status"], "published");
        assert_eq!(v["groups"], 1);
        let second = super::p2p_moq_publish_group("ffi-test-stream".to_string(), encoded).unwrap();
        let v: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert_eq!(v["groups"], 2);
    }

    #[test]
    fn test_swarm_download_validation() {
        let e = super::p2p_swarm_download(
            "not json".to_string(),
            "[]".to_string(),
            "[]".to_string(),
            String::new(),
            1,
        )
        .unwrap_err();
        assert!(e.contains("bad manifest"), "got {e}");
        let invalid = serde_json::json!({
            "blob_hash": "abc",
            "total_size": 10,
            "chunks": [{"blake3": "xyz", "offset": 0, "len": 10}]
        })
        .to_string();
        let e = super::p2p_swarm_download(
            invalid,
            "[]".to_string(),
            "[]".to_string(),
            String::new(),
            1,
        )
        .unwrap_err();
        assert!(e.contains("manifest failed validation"), "got {e}");
        let e = super::p2p_swarm_download(
            valid_manifest_json(),
            "nope".to_string(),
            "[]".to_string(),
            String::new(),
            1,
        )
        .unwrap_err();
        assert!(e.contains("bad peers"), "got {e}");
        let e = super::p2p_swarm_download(
            valid_manifest_json(),
            "[\"nope\"]".to_string(),
            "[null]".to_string(),
            String::new(),
            1,
        )
        .unwrap_err();
        assert!(e.contains("bad peer"), "got {e}");
        let e = super::p2p_swarm_download(
            valid_manifest_json(),
            "[\"8.8.8.8:7777\"]".to_string(),
            "[null]".to_string(),
            String::new(),
            1,
        )
        .unwrap_err();
        assert!(e.contains("refusing non-private peer"), "got {e}");
    }

    #[tokio::test]
    async fn test_fetch_addr_validation_rejects_non_private() {
        let e = super::p2p_quic_fetch_chunk("nope".to_string(), "h".to_string(), 0, 0)
            .await
            .unwrap_err();
        assert!(e.contains("bad addr"), "got {e}");
        let e = super::p2p_quic_fetch_chunk("8.8.8.8:443".to_string(), "h".to_string(), 0, 0)
            .await
            .unwrap_err();
        assert!(e.contains("refusing non-private peer"), "got {e}");
        let e = super::p2p_moq_subscribe_fetch("nope".to_string(), "s".to_string(), 100)
            .await
            .unwrap_err();
        assert!(e.contains("bad addr"), "got {e}");
        let e = super::p2p_moq_subscribe_fetch("8.8.8.8:443".to_string(), "s".to_string(), 100)
            .await
            .unwrap_err();
        assert!(e.contains("refusing non-private peer"), "got {e}");
        let e = super::p2p_fetch_blob_from_peer(
            "h".to_string(),
            "nope".to_string(),
            7777,
            None,
            String::new(),
        )
        .await
        .unwrap_err();
        assert!(e.contains("bad peer ip"), "got {e}");
    }

    #[test]
    fn test_no_daemon_state_queries() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(super::p2p_lan_server_port()
            .unwrap_err()
            .contains("not running"));
        assert!(super::p2p_quic_server_port()
            .unwrap_err()
            .contains("not running"));
        assert!(super::p2p_swarm_poll("unknown".to_string())
            .unwrap_err()
            .contains("unknown download"));
        assert!(super::p2p_mdns_browse_drain().unwrap().is_empty());
    }

    #[test]
    fn test_no_daemon_stops_are_noops() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(super::p2p_mdns_advertise_stop().unwrap());
        assert!(super::p2p_mdns_browse_stop().unwrap());
        assert!(super::p2p_lan_server_stop().unwrap());
        assert!(super::p2p_quic_server_stop().unwrap());
        assert!(super::p2p_swarm_cancel("unknown".to_string()).unwrap());
        assert!(super::p2p_stop_all().unwrap());
    }

    #[test]
    fn test_power_snapshot_modes() {
        let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let full = super::p2p_power_update(true, 100, false, false).unwrap();
        assert_eq!(full.mode, "full");
        assert!(!full.paused);
        assert_eq!(full.max_parallel_uploads, 8);
        assert_eq!(full.upload_budget_bytes_per_sec, u64::MAX);
        let throttled = super::p2p_power_update(false, 60, false, false).unwrap();
        assert_eq!(throttled.mode, "throttled");
        assert!(!throttled.paused);
        assert_eq!(throttled.max_parallel_uploads, 2);
        let paused = super::p2p_power_update(false, 40, true, true).unwrap();
        assert_eq!(paused.mode, "paused");
        assert!(paused.paused);
        assert_eq!(paused.max_parallel_uploads, 0);
        assert_eq!(paused.upload_budget_bytes_per_sec, 0);
        let mode = super::p2p_power_mode().unwrap();
        assert_eq!(mode.mode, "paused");
        assert!(mode.paused);
    }

    #[test]
    fn test_swarm_status_dto_helpers() {
        let running = P2pSwarmStatusDto::running();
        assert_eq!(running.state, "running");
        assert_eq!(running.verified_chunks, 0);
        assert!(running.failed_hashes.is_empty());
        let report = SwarmReport {
            verified_chunks: 3,
            bytes_downloaded: 4096,
            failures: 2,
            failed_hashes: vec!["h1".to_string(), "h2".to_string()],
        };
        let done = P2pSwarmStatusDto::from_report(report);
        assert_eq!(done.state, "done");
        assert_eq!(done.verified_chunks, 3);
        assert_eq!(done.bytes_downloaded, 4096);
        assert_eq!(done.failures, 2);
        assert_eq!(done.failed_hashes, vec!["h1".to_string(), "h2".to_string()]);
    }

    #[test]
    fn test_p2p_state_constructor() {
        let st = P2pState::new();
        assert!(st.advertiser.is_none());
        assert!(st.browser.is_none());
        assert!(st.lan_server.is_none());
        assert!(st.quic_server.is_none());
        assert!(st.downloads.is_empty());
        assert_eq!(st.next_download, 0);
    }

    #[test]
    fn test_peer_dto_from_mdns() {
        let peer = MdnsPeer {
            pubkey: "k".to_string(),
            addr: "10.0.0.3:7777".parse().unwrap(),
            quic_port: Some(4433),
        };
        let dto = P2pPeerDto::from(peer);
        assert_eq!(dto.pubkey, "k");
        assert_eq!(dto.ip, "10.0.0.3");
        assert_eq!(dto.port, 7777);
        assert_eq!(dto.quic_port, Some(4433));
    }

    #[test]
    fn test_fountain_encode_manifest() {
        let data: Vec<u8> = (0..2000u16).map(|i| (i % 251) as u8).collect();
        let json = super::p2p_encode_fountain_payload(data, 0.3).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["total_len"], 2000);
        assert_eq!(v["symbol_size"], 1024);
        assert_eq!(v["num_source_symbols"], 2);
        assert!(v["oti_data"].is_array());
        let e = super::p2p_encode_fountain_payload(Vec::new(), 0.3).unwrap_err();
        assert!(e.contains("empty payload"), "got {e}");
    }

    #[test]
    fn test_fountain_decode_roundtrip() {
        let data: Vec<u8> = (0..5000u16).map(|i| (i % 251) as u8).collect();
        let encoded = soshal_storage_core::erasure_fountain::encode_fountain(&data, 0.5).unwrap();
        let manifest_json = serde_json::to_string(&encoded.manifest).unwrap();
        let subset = encoded.packets[0..encoded.manifest.num_source_symbols as usize + 1].to_vec();
        let b64: Vec<String> = subset
            .iter()
            .map(|p| soshal_crypto_core::base64::base64_encode_bytes(p))
            .collect();
        let packets_json = serde_json::to_string(&b64).unwrap();
        let decoded = super::p2p_decode_fountain_payload(manifest_json, packets_json).unwrap();
        assert_eq!(decoded, data);
        let e =
            super::p2p_decode_fountain_payload("nope".to_string(), "[]".to_string()).unwrap_err();
        assert!(e.contains("invalid manifest JSON"), "got {e}");
        let e = super::p2p_decode_fountain_payload(
            serde_json::to_string(&encoded.manifest).unwrap(),
            "[]".to_string(),
        )
        .unwrap_err();
        assert!(e.contains("Insufficient Fountain packets"), "got {e}");
    }
}
