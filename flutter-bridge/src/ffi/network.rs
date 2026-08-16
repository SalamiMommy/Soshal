//! Network FFI module
//!
//! Manages the shared `nostr-sdk` client/relay pool in Rust. All heavy
//! networking (WebSocket keepalive, retries, backpressure) lives in
//! nostr-sdk; Dart only passes relay URLs, filters, and event JSON.
//! I2P/Freenet daemon-awareness is a Rust-side probe; per-relay transport
//! proxying stays a desktop (Tauri) concern for now.

use flutter_rust_bridge::frb;
use nostr_sdk::client::Client;
use nostr_sdk::prelude::{Filter, SubscriptionId};
use nostr_sdk::proxy::Proxy;
use serde::{Deserialize, Serialize};
use soshal_network_core::i2p_sam::I2PSessionManager;
use soshal_network_core::transport::{TransportMode, I2P_SOCKS_PORT};
use std::net::SocketAddr;
use std::sync::Mutex;

/// Shared nostr client. Holds no keys — events published through here must
/// already be signed (`signer_sign_unsigned`).
static CLIENT: Mutex<Option<Client>> = Mutex::new(None);

/// Current transport mode for outgoing traffic (clearnet / auto / i2p).
static TRANSPORT_MODE: Mutex<TransportMode> = Mutex::new(TransportMode::Clearnet);

/// Persistent i2p SAM session shared across p2p/streaming FFI calls.
static I2P_MANAGER: Mutex<Option<I2PSessionManager>> = Mutex::new(None);

fn transport_mode() -> TransportMode {
    *TRANSPORT_MODE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether outgoing traffic should ride the local i2pd daemon right now.
fn i2p_active() -> bool {
    match transport_mode() {
        TransportMode::I2p => true,
        TransportMode::Auto => super::util::tcp_probe("127.0.0.1", 7656),
        TransportMode::Clearnet => false,
    }
}

/// SOCKS5 proxy address for the current transport, if i2p is active.
pub(super) fn i2p_socks_addr() -> Option<SocketAddr> {
    i2p_active().then_some(SocketAddr::from(([127, 0, 0, 1], I2P_SOCKS_PORT)))
}

/// Relay connection status
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RelayInfo {
    pub url: String,
    pub connected: bool,
    pub latency_ms: u32,
    pub last_event_at: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpResponseDto {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Perform a network request via the HTTP/3 & QUIC network stack.
#[frb(serialize)]
pub async fn network_fetch_http3(
    url: String,
    method: String,
    headers_json: String,
    body: Option<Vec<u8>>,
) -> Result<HttpResponseDto, String> {
    let headers: std::collections::HashMap<String, String> =
        serde_json::from_str(&headers_json).unwrap_or_default();

    let client = match i2p_socks_addr() {
        Some(addr) => soshal_network_core::http3_client::Http3Client::with_socks_proxy(Some(addr)),
        None => soshal_network_core::http3_client::Http3Client::new(),
    };
    let resp = client.request(&method, &url, headers, body).await?;
    Ok(HttpResponseDto {
        status: resp.status,
        body: resp.body,
    })
}

fn client_guard() -> std::sync::MutexGuard<'static, Option<Client>> {
    CLIENT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Initialize the relay client and connect to the given relays.
#[frb(serialize)]
pub async fn network_init_relays(relay_urls: Vec<String>) -> Result<String, String> {
    if relay_urls.is_empty() {
        return Err("no relay urls".to_string()).into();
    }
    for url in &relay_urls {
        let (valid, _blocked) = soshal_content_core::url::is_valid_relay_url(url);
        if !valid {
            return Err(format!("invalid or blocked relay URL: {url}")).into();
        }
    }
    let mut builder = Client::builder();
    if let Some(addr) = i2p_socks_addr() {
        builder = builder.proxy(Proxy::all(addr));
    }
    let client = builder.build();
    let mut added = 0usize;
    for url in &relay_urls {
        if let Ok(target) = nostr::types::RelayUrl::parse(url) {
            match client.add_relay(target).await {
                Ok(_) => added += 1,
                Err(e) => return Err(format!("failed to add relay {url}: {e}")).into(),
            }
        }
    }
    if added == 0 {
        return Err("no relays could be added".to_string()).into();
    }
    let _ = client.connect().await;
    *client_guard() = Some(client.clone());
    Ok(format!("relay client initialized ({added} relays)")).into()
}

/// Add and connect a single relay.
#[frb(serialize)]
pub async fn network_add_relay(url: String) -> Result<bool, String> {
    let (valid, _blocked) = soshal_content_core::url::is_valid_relay_url(&url);
    if !valid {
        return Err("invalid or blocked relay URL".to_string()).into();
    }
    let target = match nostr::types::RelayUrl::parse(&url) {
        Ok(t) => t,
        Err(e) => return Err(format!("invalid relay URL: {e}")).into(),
    };
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    Ok(client.add_relay(target).await.is_ok()).into()
}

/// Remove (disconnect) a relay.
#[frb(serialize)]
pub async fn network_remove_relay(url: String) -> Result<bool, String> {
    let target = match nostr::types::RelayUrl::parse(&url) {
        Ok(t) => t,
        Err(e) => return Err(format!("invalid relay URL: {e}")).into(),
    };
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    Ok(client.remove_relay(target).await.is_ok()).into()
}

/// Snapshot of relay connection state (url, connected, latency, last event).
#[frb(serialize)]
pub async fn network_get_relay_status() -> Result<String, String> {
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    {
        let relays = client.relays().await;
        let mut out: Vec<RelayInfo> = relays
            .into_iter()
            .map(|(url, relay)| RelayInfo {
                url: url.to_string(),
                connected: relay.status().is_connected(),
                latency_ms: relay
                    .stats()
                    .latency()
                    .map(|l| l.as_millis().min(u64::MAX as u128) as u32)
                    .unwrap_or(0),
                last_event_at: relay.stats().connected_at().as_secs(),
            })
            .collect();
        out.sort_by(|a, b| a.url.cmp(&b.url));
        super::util::json_ok(out)
    }
}

/// Summary of relay connectivity: `{connected, total}` — drives the app-wide
/// offline banner. One relay connected means we are online.
#[frb(serialize)]
pub async fn network_relay_connection_status() -> Result<String, String> {
    let raw = network_get_relay_status().await?;
    let relays: Vec<serde_json::Value> =
        serde_json::from_str(&raw).map_err(|e| format!("parse relay status: {e}"))?;
    let connected = relays
        .iter()
        .filter(|r| r["connected"].as_bool().unwrap_or(false))
        .count();
    super::util::json_ok(serde_json::json!({
        "connected": connected,
        "total": relays.len(),
    }))
}

/// Subscribe to events matching a NIP-01 filter JSON object. Returns the
/// subscription id; events are polled with `network_take_events`.
#[frb(serialize)]
pub async fn network_subscribe(filter_json: String) -> Result<String, String> {
    let filter: Filter = match serde_json::from_str(&filter_json) {
        Ok(f) => f,
        Err(e) => return Err(format!("invalid filter JSON: {e}")),
    };
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    let sub_id = SubscriptionId::generate();
    match client.subscribe(vec![filter]).with_id(sub_id.clone()).await {
        Ok(_) => Ok(sub_id.to_string()).into(),
        Err(e) => Err(format!("subscribe failed: {e}")).into(),
    }
}

/// Unsubscribe a subscription id.
#[frb(serialize)]
pub async fn network_unsubscribe(subscription_id: String) -> Result<bool, String> {
    let sub_id = SubscriptionId::new(subscription_id);
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    match client.unsubscribe(&sub_id).await {
        Ok(_) => Ok(true).into(),
        Err(e) => Err(format!("unsubscribe failed: {e}")).into(),
    }
}

/// Publish an already-signed event (JSON) to all connected relays.
/// Returns the number of relays that accepted it.
#[frb(serialize)]
pub async fn network_publish_event(event_json: String) -> Result<i32, String> {
    let event = match serde_json::from_str::<nostr::event::Event>(&event_json) {
        Ok(e) => e,
        Err(e) => return Err(format!("invalid event JSON: {e}")),
    };
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    match client.send_event(&event).await {
        Ok(out) => Ok(out.success.len() as i32).into(),
        Err(e) => Err(format!("publish failed: {e}")).into(),
    }
}

/// Query events matching a filter against relays and the local cache.
/// Returns an array of event JSON objects.
#[frb(serialize)]
pub async fn network_query_events(filter_json: String) -> Result<String, String> {
    let filter: Filter = match serde_json::from_str(&filter_json) {
        Ok(f) => f,
        Err(e) => return Err(format!("invalid filter JSON: {e}")),
    };
    let client = match client_guard().as_ref() {
        Some(c) => c.clone(),
        None => return Err("relay client not initialized".to_string()).into(),
    };
    {
        match client.fetch_events(vec![filter]).await {
            Ok(events) => match serde_json::to_string(&events) {
                Ok(json) => Ok(json).into(),
                Err(e) => Err(format!("serialize: {e}")).into(),
            },
            Err(e) => Err(format!("query failed: {e}")).into(),
        }
    }
}

/// Raw TCP probe: is the local i2pd SOCKS proxy (port 7656, `.i2p` reach)
/// listening? Exposed so the Flutter UI can show tunnel status.
#[frb(serialize)]
pub fn network_i2p_status() -> Result<bool, String> {
    Ok(super::util::tcp_probe("127.0.0.1", 7656)).into()
}

/// Raw TCP probe: is a local Freenet gateway (default HTTP port 8888)
/// listening?
#[frb(serialize)]
pub fn network_freenet_status() -> Result<bool, String> {
    Ok(super::util::tcp_probe("127.0.0.1", 8888)).into()
}

/// Current transport mode: `"clearnet"`, `"auto"`, or `"i2p"`.
#[frb(sync, serialize)]
pub fn network_get_transport_mode() -> Result<String, String> {
    Ok(transport_mode().as_str().to_string()).into()
}

/// Sets the transport mode for all outgoing traffic. Persist in Dart
/// (settings KV) and re-init relays for the proxy to take effect.
#[frb(sync, serialize)]
pub fn network_set_transport_mode(mode: String) -> Result<bool, String> {
    match TransportMode::parse_mode(&mode) {
        Some(m) => {
            *TRANSPORT_MODE.lock().unwrap_or_else(|e| e.into_inner()) = m;
            Ok(true).into()
        }
        None => Err(format!("unknown transport mode: {mode}")).into(),
    }
}

// ---------------------------------------------------------------------------
// Reticulum FFI functions
// ---------------------------------------------------------------------------

/// Starts Reticulum UDP transport on the specified bind address
#[frb(sync, serialize)]
pub fn reticulum_start_transport(pubkey: String, bind_addr: String) -> Result<String, String> {
    use soshal_network_core::reticulum::node_for;

    let node = node_for(&pubkey)?;
    let mut node = node.lock().unwrap_or_else(|e| e.into_inner());
    node.start_udp_transport(&bind_addr)
        .map_err(|e| format!("Failed to start Reticulum transport: {e}"))?;

    let status = node.get_status();
    serde_json::to_string(&status).map_err(|e| format!("Failed to serialize status: {e}"))
}

/// Starts Reticulum AutoInterface for peer discovery
#[frb(sync, serialize)]
pub fn reticulum_start_auto_interface(
    pubkey: String,
    enabled: bool,
    port: u16,
    interval_ms: u64,
) -> Result<String, String> {
    use soshal_network_core::reticulum::{node_for, AutoInterfaceConfig};

    let node = node_for(&pubkey)?;
    let mut node = node.lock().unwrap_or_else(|e| e.into_inner());
    let config = AutoInterfaceConfig {
        enabled,
        bind_port: port,
        discovery_interval_ms: interval_ms,
    };

    node.start_auto_interface(config)
        .map_err(|e| format!("Failed to start AutoInterface: {e}"))?;

    let status = node.get_status();
    serde_json::to_string(&status).map_err(|e| format!("Failed to serialize status: {e}"))
}

/// Starts Reticulum TCP server interface
#[frb(sync, serialize)]
pub fn reticulum_start_tcp_server(
    pubkey: String,
    port: u16,
    max_connections: usize,
) -> Result<String, String> {
    use soshal_network_core::reticulum::{node_for, TcpInterfaceConfig};

    let node = node_for(&pubkey)?;
    let mut node = node.lock().unwrap_or_else(|e| e.into_inner());
    let config = TcpInterfaceConfig {
        listen_port: port,
        max_connections,
        enabled: true,
    };

    node.start_tcp_server(config)
        .map_err(|e| format!("Failed to start TCP server: {e}"))?;

    let status = node.get_status();
    serde_json::to_string(&status).map_err(|e| format!("Failed to serialize status: {e}"))
}

/// Sends a Reticulum packet to the specified destination
#[frb(sync, serialize)]
pub fn reticulum_send_packet(
    pubkey: String,
    dest_addr: String,
    packet_json: String,
) -> Result<bool, String> {
    use soshal_network_core::reticulum::{node_for, ReticulumPacket};

    let node = node_for(&pubkey)?;
    let node = node.lock().unwrap_or_else(|e| e.into_inner());
    let dest_socket = dest_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("Invalid destination address: {e}"))?;

    let packet: ReticulumPacket =
        serde_json::from_str(&packet_json).map_err(|e| format!("Invalid packet JSON: {e}"))?;

    node.send_packet(dest_socket, &packet)
        .map_err(|e| format!("Failed to send packet: {e}"))?;

    Ok(true)
}

/// Creates a Reticulum link request to a remote destination
#[frb(sync, serialize)]
pub fn reticulum_request_link(_pubkey: String, dest_hex: String) -> Result<String, String> {
    use soshal_network_core::reticulum::{LinkManager, ReticulumAddress};

    let dest = ReticulumAddress::from_hex(&dest_hex)
        .map_err(|e| format!("Invalid destination hex: {e}"))?;

    let manager = LinkManager::new();
    let packet = manager
        .request_link(dest)
        .map_err(|e| format!("Failed to request link: {e}"))?;

    serde_json::to_string(&packet).map_err(|e| format!("Failed to serialize packet: {e}"))
}

/// Gets current Reticulum node status
#[frb(sync, serialize)]
pub fn reticulum_get_status(pubkey: String) -> Result<String, String> {
    use soshal_network_core::reticulum::node_for;

    let node = node_for(&pubkey)?;
    let node = node.lock().unwrap_or_else(|e| e.into_inner());
    let status = node.get_status();

    serde_json::to_string(&status).map_err(|e| format!("Failed to serialize status: {e}"))
}

/// Creates a Reticulum address from a public key
#[frb(sync, serialize)]
pub fn reticulum_address_from_pubkey(pubkey: String) -> Result<String, String> {
    use soshal_network_core::reticulum::ReticulumAddress;

    let addr = ReticulumAddress::from_pubkey(&pubkey);
    serde_json::to_string(&addr).map_err(|e| format!("Failed to serialize address: {e}"))
}

/// Creates a Reticulum address from an app name and aspect
#[frb(sync, serialize)]
pub fn reticulum_address_from_aspect(app_name: String, aspect: String) -> Result<String, String> {
    use soshal_network_core::reticulum::ReticulumAddress;

    let addr = ReticulumAddress::from_aspect(&app_name, &aspect);
    serde_json::to_string(&addr).map_err(|e| format!("Failed to serialize address: {e}"))
}

/// Broadcasts an ANNOUNCE packet for identity discovery
#[frb(sync, serialize)]
pub fn reticulum_create_announce(pubkey: String, aspect: Option<String>) -> Result<String, String> {
    use soshal_network_core::reticulum::node_for;

    let node = node_for(&pubkey)?;
    let node = node.lock().unwrap_or_else(|e| e.into_inner());
    let packet = node.create_announce(aspect.as_deref());

    serde_json::to_string(&packet).map_err(|e| format!("Failed to serialize packet: {e}"))
}

// ---------------------------------------------------------------------------
// Freenet FFI functions
// ---------------------------------------------------------------------------

/// Connects to a Freenet node via WebSocket
#[frb(serialize)]
pub async fn freenet_connect(url: String, auth_token: String) -> Result<bool, String> {
    use soshal_network_core::freenet_websocket::FreenetWebSocketClient;

    let client = FreenetWebSocketClient::new(url, auth_token);
    client
        .connect()
        .await
        .map_err(|e| format!("Freenet connection failed: {e}"))?;

    Ok(true)
}

/// Fetches contract state from Freenet
#[frb(serialize)]
pub async fn freenet_get_contract(
    url: String,
    auth_token: String,
    key: String,
    subscribe: bool,
) -> Result<String, String> {
    use soshal_network_core::freenet_websocket::FreenetWebSocketClient;

    let client = FreenetWebSocketClient::new(url, auth_token);
    client.connect().await?;

    let state = client
        .get_contract(&key, subscribe)
        .await
        .map_err(|e| format!("Get contract failed: {e}"))?;

    serde_json::to_string(&state).map_err(|e| format!("Failed to serialize state: {e}"))
}

/// Publishes contract state to Freenet
#[frb(serialize)]
pub async fn freenet_put_contract(
    url: String,
    auth_token: String,
    state_json: String,
    subscribe: bool,
) -> Result<String, String> {
    use soshal_network_core::freenet_websocket::{ContractState, FreenetWebSocketClient};

    let state: ContractState =
        serde_json::from_str(&state_json).map_err(|e| format!("Invalid state JSON: {e}"))?;

    let client = FreenetWebSocketClient::new(url, auth_token);
    client.connect().await?;

    let key = client
        .put_contract(state, subscribe)
        .await
        .map_err(|e| format!("Put contract failed: {e}"))?;

    Ok(key)
}

/// Subscribes to Freenet contract updates
#[frb(serialize)]
pub async fn freenet_subscribe(
    url: String,
    auth_token: String,
    key: String,
    summary_json: Option<String>,
) -> Result<bool, String> {
    use soshal_network_core::freenet_websocket::{ContractSummary, FreenetWebSocketClient};

    let summary = if let Some(summary_str) = summary_json {
        Some(
            serde_json::from_str::<ContractSummary>(&summary_str)
                .map_err(|e| format!("Invalid summary JSON: {e}"))?,
        )
    } else {
        None
    };

    let client = FreenetWebSocketClient::new(url, auth_token);
    client.connect().await?;

    client
        .subscribe_contract(&key, summary)
        .await
        .map_err(|e| format!("Subscribe failed: {e}"))?;

    Ok(true)
}

// ---------------------------------------------------------------------------
// I2P FFI functions
// ---------------------------------------------------------------------------

fn connect_i2p(
    sam_host: String,
    sam_port: u16,
) -> Result<soshal_network_core::i2p_sam::I2PSamClient, String> {
    let mut client = soshal_network_core::i2p_sam::I2PSamClient::new(sam_host, sam_port);
    client
        .connect()
        .map_err(|e| format!("I2P connection failed: {e}"))?;
    client
        .handshake()
        .map_err(|e| format!("I2P handshake failed: {e}"))?;
    Ok(client)
}

/// Connects to I2P SAM bridge
#[frb(sync, serialize)]
pub fn i2p_connect(sam_host: String, sam_port: u16) -> Result<bool, String> {
    connect_i2p(sam_host, sam_port)?;

    Ok(true)
}

/// Creates an I2P session
#[frb(sync, serialize)]
pub fn i2p_create_session(
    sam_host: String,
    sam_port: u16,
    session_id: String,
    destination: Option<String>,
) -> Result<String, String> {
    let mut client = connect_i2p(sam_host, sam_port)?;

    let response = client
        .create_session(&session_id, destination.as_deref())
        .map_err(|e| format!("Session creation failed: {e}"))?;

    Ok(response)
}

/// Generates a new I2P destination
#[frb(sync, serialize)]
pub fn i2p_generate_destination(sam_host: String, sam_port: u16) -> Result<String, String> {
    let mut client = connect_i2p(sam_host, sam_port)?;

    let destination = client
        .generate_destination()
        .map_err(|e| format!("Destination generation failed: {e}"))?;

    Ok(destination)
}

/// Connects to a remote I2P destination
#[frb(sync, serialize)]
pub fn i2p_connect_to_destination(
    sam_host: String,
    sam_port: u16,
    session_id: String,
    destination: String,
) -> Result<bool, String> {
    let mut client = connect_i2p(sam_host, sam_port)?;

    client
        .create_session(&session_id, None)
        .map_err(|e| format!("Session creation failed: {e}"))?;

    client
        .connect_to_destination(&destination)
        .map_err(|e| format!("Destination connection failed: {e}"))?;

    Ok(true)
}

/// Starts the persistent i2p session (SAM). Returns the session destination;
/// persist it and pass it back on later runs to keep a stable address.
#[frb(sync, serialize)]
pub fn i2p_start_session(destination: Option<String>) -> Result<String, String> {
    let mut guard = I2P_MANAGER.lock().unwrap_or_else(|e| e.into_inner());
    let manager = guard.get_or_insert_with(I2PSessionManager::new);
    manager.start(destination.as_deref())
}

/// Stops the persistent i2p session (if any).
#[frb(sync, serialize)]
pub fn i2p_stop_session() -> Result<bool, String> {
    let mut guard = I2P_MANAGER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = guard.as_mut() {
        m.stop();
        Ok(true).into()
    } else {
        Ok(false).into()
    }
}

/// `{"running": bool, "destination": string|null}` for the i2p session.
#[frb(sync, serialize)]
pub fn i2p_session_status() -> Result<String, String> {
    let guard = I2P_MANAGER.lock().unwrap_or_else(|e| e.into_inner());
    let running = guard.as_ref().map(|m| m.is_running()).unwrap_or(false);
    let destination = guard.as_ref().and_then(|m| m.destination());
    super::util::json_ok(serde_json::json!({
        "running": running,
        "destination": destination,
    }))
}

/// Retrieves multi-bearer off-grid mesh connection status.
#[frb(serialize)]
pub async fn network_get_multi_bearer_status(own_pubkey: String) -> Result<String, String> {
    let actor = soshal_network_core::multi_bearer::MultiBearerActor::new(&own_pubkey, [0u8; 32]);
    let state = actor.get_state().await;
    super::util::json_ok(state)
}

/// Process incoming BLE state root beacon and determine if Wi-Fi Direct socket upgrade is needed.
#[frb(serialize)]
pub async fn network_process_ble_beacon(
    beacon: String,
    local_root_hex: String,
    own_pubkey: String,
) -> Result<Option<String>, String> {
    let actor = soshal_network_core::multi_bearer::MultiBearerActor::new(&own_pubkey, [0u8; 32]);
    Ok(actor.process_ble_beacon(&beacon, &local_root_hex).await)
}

/// Reconciles Prolly Tree root hashes with a remote peer.
#[frb(serialize)]
pub fn network_reconcile_prolly_tree(
    local_kv_json: String,
    remote_root_hash: String,
) -> Result<String, String> {
    let kv_pairs: Vec<(String, String)> = serde_json::from_str(&local_kv_json).unwrap_or_default();
    let tree = soshal_sync_core::prolly_tree::ProllyTree::build(&kv_pairs);
    let mut session = soshal_sync_core::prolly_sync::ProllySyncSession::new(tree);
    let resp = session.handle_message(
        soshal_sync_core::prolly_sync::ProllySyncMessage::RootExchange {
            root_hash: remote_root_hash,
        },
    );
    super::util::json_ok(resp)
}

/// Verifies a zk-SNARK Web-of-Trust moderation proof.
#[frb(serialize)]
pub fn network_verify_zk_wot_proof(
    proof_json: String,
    expected_wot_root: String,
    blacklisted_nullifiers_json: String,
) -> Result<bool, String> {
    let nullifiers: Vec<String> =
        serde_json::from_str(&blacklisted_nullifiers_json).unwrap_or_default();
    Ok(soshal_moderation_core::check::check_zk_trust_proof(
        &proof_json,
        &expected_wot_root,
        &nullifiers,
    ))
}

/// Notifies network-core of local IP interface address changes to trigger QUIC connection migration.
#[frb(sync, serialize)]
pub fn network_notify_interface_change(new_ip: String) -> Result<bool, String> {
    let mgr = soshal_network_core::quic_migration::QuicMigrationManager::new();
    if let Ok(addr) = new_ip.parse::<std::net::SocketAddr>() {
        mgr.handle_interface_change(addr)?;
        Ok(true)
    } else {
        Err(format!("Invalid IP address format: {}", new_ip))
    }
}

/// Retrieves real-time kernel, hardware crypto, io_uring, and Cuckoo filter diagnostics.
#[frb(sync, serialize)]
pub fn network_get_sys_diagnostics() -> Result<String, String> {
    let crypto_caps = soshal_crypto_core::hardware_accel::probe_crypto_hardware();
    let cpu_topology = soshal_common_core::thread_governor::CpuTopology::discover();
    let io_engine = soshal_storage_core::io_uring_backend::IoUringEngine::new();

    let diag = serde_json::json!({
        "hardware_crypto": crypto_caps,
        "cpu_topology": cpu_topology,
        "io_engine_mode": format!("{:?}", io_engine.mode),
        "schema_version": soshal_db_core::schema::SCHEMA_VERSION,
    });

    super::util::json_ok(diag)
}

/// Reticulum node state (singleton)
static RETICULUM: Mutex<Option<soshal_network_core::reticulum::ReticulumNode>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReticulumStatusDto {
    pub running: bool,
    pub destination_hash: String,
    pub active_routes: usize,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

/// Stop Reticulum transport.
#[frb(sync, serialize)]
pub fn network_reticulum_stop() -> Result<bool, String> {
    let mut guard = RETICULUM.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
    Ok(true)
}

/// Get current Reticulum status.
#[frb(sync, serialize)]
pub fn network_reticulum_status() -> Result<String, String> {
    let guard = RETICULUM.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(node) => {
            let running = *node.running.lock().unwrap_or_else(|e| e.into_inner());
            let rx = *node.rx_count.lock().unwrap_or_else(|e| e.into_inner());
            let tx = *node.tx_count.lock().unwrap_or_else(|e| e.into_inner());
            let routes = node
                .path_table
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len();

            let status = ReticulumStatusDto {
                running,
                destination_hash: node.destination.to_hex(),
                active_routes: routes,
                rx_packets: rx,
                tx_packets: tx,
            };
            serde_json::to_string(&status).map_err(|e| format!("serialize status: {e}"))
        }
        None => {
            let status = ReticulumStatusDto {
                running: false,
                destination_hash: String::new(),
                active_routes: 0,
                rx_packets: 0,
                tx_packets: 0,
            };
            serde_json::to_string(&status).map_err(|e| format!("serialize status: {e}"))
        }
    }
}

/// Announce the active Reticulum destination.
#[frb(sync, serialize)]
pub fn network_reticulum_announce(_pubkey: String) -> Result<bool, String> {
    let guard = RETICULUM.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(node) => {
            if !*node.running.lock().unwrap_or_else(|e| e.into_inner()) {
                return Err("Reticulum not running".to_string());
            }
            // Announce via LinkManager
            node.link_manager
                .announce(&"".to_string())
                .map_err(|e| format!("Announce failed: {e}"))
        }
        None => Err("Reticulum not initialized".to_string()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_validate_relay_urls_rejected() {
        let (valid, _) = soshal_content_core::url::is_valid_relay_url("http://10.0.0.1:7777");
        assert!(!valid);
        let (valid, _) = soshal_content_core::url::is_valid_relay_url("wss://relay.damus.io");
        assert!(valid);
    }

    #[test]
    fn test_sys_diagnostics_shape() {
        let diag = super::network_get_sys_diagnostics().unwrap();
        let v: serde_json::Value = serde_json::from_str(&diag).unwrap();
        assert!(v["schema_version"].as_i64().unwrap() > 0);
        assert!(v["io_engine_mode"].is_string());
        assert!(v["cpu_topology"].is_object());
        assert!(v["hardware_crypto"].is_object());
    }

    #[test]
    fn test_notify_interface_change_rejects_bad_addr() {
        let e = super::network_notify_interface_change("10.0.0.1".to_string()).unwrap_err();
        assert!(e.contains("Invalid IP address format"), "got {e}");
        assert!(super::network_notify_interface_change("10.0.0.1:9999".to_string()).is_ok());
    }

    #[test]
    fn test_transport_mode_roundtrip() {
        for name in ["clearnet", "auto", "i2p"] {
            assert!(super::network_set_transport_mode(name.to_string()).unwrap());
            assert_eq!(super::network_get_transport_mode().unwrap(), name);
        }
        assert!(super::network_set_transport_mode("bogus".to_string()).is_err());
        assert!(super::network_set_transport_mode("clearnet".to_string()).unwrap());
    }

    #[tokio::test]
    async fn test_multi_bearer_state_pure() {
        let state = super::network_get_multi_bearer_status("pk_test_1".to_string())
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&state).unwrap();
        assert_eq!(v["own_pubkey"], "pk_test_1");
        assert_eq!(v["ble_active"], true);
        assert_eq!(v["wifi_direct_connected"], false);
    }

    #[tokio::test]
    async fn test_ble_beacon_pure_logic() {
        let dev = soshal_network_core::ble::device_name("abcdef1234567890");
        let beacon = format!("{dev}:deadbeefcafe0000");
        let call = |b: &str, local: &str| {
            super::network_process_ble_beacon(b.to_string(), local.to_string(), "pk_x".to_string())
        };
        assert_eq!(
            call(&beacon, "0000000000000000").await.unwrap(),
            Some("abcdef123456".to_string())
        );
        assert_eq!(call(&beacon, "deadbeefcafe0000").await.unwrap(), None);
        assert_eq!(call("malformed", "0000000000000000").await.unwrap(), None);
        assert_eq!(
            call("other:deadbeefcafe0000", "0000000000000000")
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn test_prolly_tree_reconcile_pure() {
        let kv = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        let tree = soshal_sync_core::prolly_tree::ProllyTree::build(&kv);
        let kv_json = serde_json::to_string(&kv).unwrap();
        let ok = super::network_reconcile_prolly_tree(kv_json.clone(), tree.root_hash).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ok)
                .unwrap()
                .as_str(),
            Some("Match")
        );
        let diff =
            super::network_reconcile_prolly_tree(kv_json, "different-root".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&diff).unwrap();
        assert_eq!(v["RequestBranch"]["node_hash"], "different-root");
        assert_eq!(v["RequestBranch"]["level"], 0);
    }

    #[test]
    fn test_zk_wot_proof_rejects_garbage() {
        assert!(!super::network_verify_zk_wot_proof(
            "".to_string(),
            "root".to_string(),
            "[]".to_string()
        )
        .unwrap());
        assert!(!super::network_verify_zk_wot_proof(
            "not json".to_string(),
            "root".to_string(),
            "[]".to_string()
        )
        .unwrap());
        assert!(!super::network_verify_zk_wot_proof(
            "{}".to_string(),
            "root".to_string(),
            "[\"n1\"]".to_string()
        )
        .unwrap());
    }

    #[test]
    fn test_skademlia_node_id_pow() {
        let mut found = None;
        for nonce in 0..1_000_000u64 {
            if let Some(id) = super::network_skademlia_generate_node_id(
                "pow_test_pubkey".to_string(),
                nonce,
                nonce,
            )
            .unwrap()
            {
                found = Some(id);
                break;
            }
        }
        assert_eq!(found.expect("pow nonce found").len(), 64);
    }

    #[test]
    fn test_reticulum_address_derivation() {
        let a1 = super::reticulum_address_from_pubkey("npub_test".to_string()).unwrap();
        let a2 = super::reticulum_address_from_pubkey("npub_test".to_string()).unwrap();
        let a3 = super::reticulum_address_from_pubkey("npub_other".to_string()).unwrap();
        assert_eq!(a1, a2);
        assert_ne!(a1, a3);
        let v: serde_json::Value = serde_json::from_str(&a1).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 16);
        let aspect =
            super::reticulum_address_from_aspect("soshal.app".to_string(), "feed".to_string())
                .unwrap();
        let v2: serde_json::Value = serde_json::from_str(&aspect).unwrap();
        assert_eq!(v2.as_array().unwrap().len(), 16);
    }

    #[test]
    fn test_reticulum_request_link_pure() {
        let dest = soshal_network_core::reticulum::address::ReticulumAddress::from_aspect(
            "soshal.app",
            "feed",
        );
        let json = super::reticulum_request_link("ignored".to_string(), dest.to_hex()).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .is_object());
    }

    #[test]
    fn test_i2p_session_idle() {
        assert!(!super::i2p_stop_session().unwrap());
        let v: serde_json::Value =
            serde_json::from_str(&super::i2p_session_status().unwrap()).unwrap();
        assert_eq!(v["running"], false);
        assert!(v["destination"].is_null());
    }

    #[test]
    fn test_reticulum_static_idle() {
        assert!(super::network_reticulum_stop().unwrap());
        let v: serde_json::Value =
            serde_json::from_str(&super::network_reticulum_status().unwrap()).unwrap();
        assert_eq!(v["running"], false);
        assert!(super::network_reticulum_prune_stale_links().is_err());
        assert!(super::network_reticulum_prune_routes(0).is_err());
        assert!(super::network_reticulum_announce("pk".to_string()).is_err());
        soshal_network_core::reticulum::transport::reset_nodes();
    }

    #[tokio::test]
    async fn test_relay_error_paths_without_client() {
        assert!(super::network_init_relays(vec![]).await.is_err());
        assert!(
            super::network_init_relays(vec!["http://10.0.0.1:7777".to_string()])
                .await
                .is_err()
        );
        assert!(super::network_add_relay("http://10.0.0.1:7777".to_string())
            .await
            .is_err());
        let e = super::network_add_relay("wss://relay.damus.io".to_string())
            .await
            .unwrap_err();
        assert!(e.contains("not initialized"), "got {e}");
        assert!(super::network_remove_relay("not a url".to_string())
            .await
            .is_err());
        assert!(super::network_get_relay_status().await.is_err());
        assert!(super::network_relay_connection_status().await.is_err());
        assert!(super::network_subscribe("not json".to_string())
            .await
            .is_err());
        assert!(super::network_subscribe("{\"kinds\":[1]}".to_string())
            .await
            .is_err());
        assert!(super::network_query_events("not json".to_string())
            .await
            .is_err());
        assert!(super::network_publish_event("not json".to_string())
            .await
            .is_err());
        assert!(super::network_unsubscribe("any".to_string()).await.is_err());
    }
}

/// Prune stale Reticulum links; returns count removed.
#[frb(sync, serialize)]
pub fn network_reticulum_prune_stale_links() -> Result<usize, String> {
    let guard = RETICULUM.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(node) => Ok(node.link_manager.prune_stale_links()),
        None => Err("Reticulum not initialized".to_string()),
    }
}

/// Drop expired Reticulum path entries; returns count removed.
#[frb(sync, serialize)]
pub fn network_reticulum_prune_routes(now_secs: u64) -> Result<usize, String> {
    let guard = RETICULUM.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(node) => {
            let mut table = node.path_table.lock().unwrap_or_else(|e| e.into_inner());
            Ok(table.prune_expired(now_secs))
        }
        None => Err("Reticulum not initialized".to_string()),
    }
}

/// Proof-of-work node id for a pubkey (hex, `null` when the static nonce fails).
#[frb(sync, serialize)]
pub fn network_skademlia_generate_node_id(
    pubkey: String,
    static_nonce: u64,
    dynamic_nonce: u64,
) -> Result<Option<String>, String> {
    let id = soshal_network_core::skademlia::generate_node_id(&pubkey, static_nonce, dynamic_nonce);
    Ok(id.map(|n| hex::encode(n)))
}
