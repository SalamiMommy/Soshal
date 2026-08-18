//! Mesh relay node FFI module
//!
//! Thin adapter over `soshal-relay-core`: starts/stops the device-as-relay
//! node (Reticulum + I2P + Freenet backends), drains inbound envelopes,
//! verifies + ingests them through the sync-core path, and exposes a
//! publish helper for `publish_or_enqueue`.

use flutter_rust_bridge::frb;
use nostr::event::Event;
use soshal_relay_core::backends::freenet::FreenetBackend;
use soshal_relay_core::backends::i2p::I2pBackend;
use soshal_relay_core::backends::reticulum::ReticulumBackend;
use soshal_relay_core::relay::RelayNode;
use soshal_sync_core::SyncUpdate;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// The running relay node (None when stopped).
static NODE: Mutex<Option<RelayNode>> = Mutex::new(None);

/// Whether the mesh ingest task is alive.
static MESH_INGEST_ALIVE: AtomicBool = AtomicBool::new(false);

fn node_guard() -> std::sync::MutexGuard<'static, Option<RelayNode>> {
    NODE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the mesh relay node is running.
pub(super) fn mesh_running() -> bool {
    node_guard().as_ref().map(|n| n.running()).unwrap_or(false)
}

/// Start the mesh relay node: Reticulum (for the active pubkey), I2P SAM,
/// Freenet (gated on a local node). Starts the inbound ingest task that
/// polls the node, verifies envelopes, and routes events into sync-core.
/// Returns JSON status.
#[frb(sync, serialize)]
pub fn relay_node_start(pubkey: String) -> Result<String, String> {
    if node_guard().is_some() {
        let _ = relay_node_stop();
    }
    let mut node = RelayNode::new();
    node.add_backend(Box::new(ReticulumBackend::new(pubkey)));
    node.add_backend(Box::new(I2pBackend::new()));
    node.add_backend(Box::new(FreenetBackend::new()));
    node.start()
        .map_err(|e| format!("mesh relay start failed: {e}"))?;
    *node_guard() = Some(node);

    let db_path = super::db::db_path()?;
    let my_pubkey = super::signer::signer_pubkey()?;
    spawn_ingest(db_path, my_pubkey);
    Ok(status())
}

/// Stop the mesh relay node and its ingest task.
#[frb(sync, serialize)]
pub fn relay_node_stop() -> Result<bool, String> {
    if let Some(mut node) = node_guard().take() {
        node.stop();
    }
    MESH_INGEST_ALIVE.store(false, Ordering::Relaxed);
    Ok(true).into()
}

/// JSON status: `{running, peers:{kind:n}, published, received, delivered}`.
#[frb(sync, serialize)]
pub fn relay_node_status() -> Result<String, String> {
    Ok(status()).into()
}

/// Connect an I2P peer by destination hash (best-effort).
#[frb(sync, serialize)]
pub fn relay_connect_i2p(destination: String) -> Result<bool, String> {
    let mut guard = node_guard();
    let Some(node) = guard.as_mut() else {
        return Err("mesh relay not running".to_string()).into();
    };
    node.connect_i2p(&destination)
        .map_err(|e| format!("i2p connect failed: {e}"))?;
    Ok(true).into()
}

/// Publish a signed event JSON to the mesh. Returns Ok(true) when published
/// via mesh, Ok(false) when the mesh is not the active transport (caller
/// falls back to relays/outbox).
pub(super) fn try_mesh_publish(event_json: &str) -> Result<bool, String> {
    let (kind, _) = super::network::resolved_kind();
    if kind == soshal_network_core::transport::TransportKind::Nostr {
        return Ok(false);
    }
    let mut guard = node_guard();
    let Some(node) = guard.as_mut() else {
        return Ok(false);
    };
    if !node.running() {
        return Ok(false);
    }
    let event: Event = Event::from_json(event_json)
        .map_err(|e| format!("mesh publish: invalid event JSON: {e}"))?;
    node.poll();
    node.publish(
        &event.id.to_hex(),
        event.kind.as_u16(),
        &event.pubkey.to_hex(),
        event.created_at.as_secs(),
        event_json.as_bytes().to_vec(),
    )
    .map(|_| true)
    .map_err(|e| format!("mesh publish failed: {e}"))
}

fn status() -> String {
    let mut guard = node_guard();
    match guard.as_mut() {
        Some(node) => node.status(),
        None => serde_json::json!({
            "running": false,
            "peers": {},
            "published": 0,
            "received": 0,
            "delivered": 0,
        })
        .to_string(),
    }
}

/// Poll the node + ingest verified events into sync-core (DB cache + app
/// stream), mirroring the sync engine's relay ingest path.
fn spawn_ingest(db_path: String, my_pubkey: String) {
    if MESH_INGEST_ALIVE.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => return,
        };
        rt.block_on(async move {
            let db = match soshal_db_core::Database::open(&db_path) {
                Ok(db) => db,
                Err(_) => return,
            };
            let (tx, mut rx) = tokio::sync::mpsc::channel::<SyncUpdate>(256);
            let forwarder = tokio::spawn(async move {
                while let Some(update) = rx.recv().await {
                    let Some(json) = super::sync::update_json(update) else {
                        continue;
                    };
                    super::sync::push_update_to_sink(json);
                }
            });
            loop {
                if !MESH_INGEST_ALIVE.load(Ordering::Relaxed) || !mesh_running() {
                    break;
                }
                {
                    let mut guard = node_guard();
                    let Some(node) = guard.as_mut() else {
                        break;
                    };
                    node.poll();
                    for payload in node.drain_delivered() {
                        let Ok(event) = Event::from_json(&payload) else {
                            continue;
                        };
                        if event.verify().is_err() {
                            continue;
                        }
                        let _ = soshal_sync_core::ingest::handle(&db, &my_pubkey, &event, &tx);
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            forwarder.abort();
            MESH_INGEST_ALIVE.store(false, Ordering::Relaxed);
        });
    });
}
