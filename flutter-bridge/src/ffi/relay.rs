//! Mesh relay node FFI module
//!
//! Thin adapter over `soshal-relay-core`: starts/stops the device-as-relay
//! node (Reticulum + I2P + Freenet backends), drains inbound envelopes,
//! verifies + ingests them through the sync-core path, and exposes a
//! publish helper for `publish_or_enqueue`.

use flutter_rust_bridge::frb;
use nostr::event::Event;
use soshal_relay_core::backends::BackendKind;
use soshal_relay_core::relay::RelayNode;
use soshal_sync_core::SyncUpdate;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// The running relay node (None when stopped).
static NODE: Mutex<Option<RelayNode>> = Mutex::new(None);

/// Whether the mesh ingest task is alive.
static MESH_INGEST_ALIVE: AtomicBool = AtomicBool::new(false);

/// Monotonically increasing generation counter. Each `spawn_ingest` call
/// increments it; the spawned thread captures its generation and exits early
/// if a newer `spawn_ingest` supersedes it (handles the stop→start race
/// within the ingest loop's 500 ms sleep).
static MESH_INGEST_GEN: AtomicU64 = AtomicU64::new(0);

fn node_guard() -> std::sync::MutexGuard<'static, Option<RelayNode>> {
    NODE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the mesh relay node is running.
pub(super) fn mesh_running() -> bool {
    node_guard().as_ref().map(|n| n.running()).unwrap_or(false)
}

/// Recent flood-gossip event payloads (newest first, capped). Backs the
/// mesh fetch path for `network_query_events` when a mesh transport is
/// resolved.
pub(super) fn mesh_recent(limit: usize) -> Vec<Vec<u8>> {
    let mut guard = node_guard();
    match guard.as_mut() {
        Some(node) => node.recent_payloads(limit),
        None => Vec::new(),
    }
}

/// Whether the mesh relay node has the given transport backend running.
/// Feeds transport resolution: a live mesh backend counts as its transport
/// being up even when the local probe (daemon port) fails.
pub(super) fn mesh_backend_up(kind: soshal_network_core::transport::TransportKind) -> bool {
    use soshal_network_core::transport::TransportKind as K;
    let guard = node_guard();
    match guard.as_ref() {
        Some(node) => node.running_backends().iter().any(|b| {
            matches!(
                (b, kind),
                (BackendKind::Reticulum, K::Reticulum)
                    | (BackendKind::I2p, K::I2p)
                    | (BackendKind::Freenet, K::Freenet)
            )
        }),
        None => false,
    }
}

/// Start the mesh relay node: backends are gated by the current transport
/// mode — "only" modes pin the single backend, `default` runs all three.
/// Starts the inbound ingest task that polls the node, verifies envelopes,
/// and routes events into sync-core. Returns JSON status.
#[frb(sync, serialize)]
pub fn relay_node_start(pubkey: String) -> Result<String, String> {
    if node_guard().is_some() {
        let _ = relay_node_stop();
    }
    let mode = super::network::transport_mode();
    let mut node = RelayNode::new_with_transport(mode, &pubkey);
    if node.running_backends().is_empty() {
        return Err("mesh relay needs a mesh transport mode (not nostr)".to_string());
    }
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
///
/// Generation counter: each call bumps `MESH_INGEST_GEN` and captures the
/// value. If a subsequent `spawn_ingest` fires while the old thread is
/// still sleeping, the old thread detects its generation is stale and
/// exits, ensuring the fresh thread (with the new account params) takes
/// over. The `MESH_INGEST_ALIVE` flag is kept for explicit stop, but the
/// thread also exits on generation mismatch or `!mesh_running()`.
fn spawn_ingest(db_path: String, my_pubkey: String) {
    let gen = MESH_INGEST_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    MESH_INGEST_ALIVE.store(true, Ordering::Relaxed);
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
                // Exit when: superseded by a newer spawn, stop requested, or
                // the relay node itself has been torn down.
                let current_gen = MESH_INGEST_GEN.load(Ordering::Relaxed);
                if current_gen != gen
                    || !MESH_INGEST_ALIVE.load(Ordering::Relaxed)
                    || !mesh_running()
                {
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
            // Only reset the alive flag if this thread is still the current
            // generation — a newer spawn owns the flag.
            if MESH_INGEST_GEN.load(Ordering::Relaxed) == gen {
                MESH_INGEST_ALIVE.store(false, Ordering::Relaxed);
            }
        });
    });
}
