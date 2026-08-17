//! Background sync FFI module
//!
//! Thin adapter over `soshal-sync-core`: starts/stops the dedicated sync
//! engine (relay WebSockets owned by Rust on their own thread+runtime) and
//! bridges its event channel to Flutter through a StreamSink. Incoming
//! kind-4 payloads are decrypted here (the only key-bearing crate) and
//! persisted via `messaging_store_dm` — mirroring the legacy sync loop that
//! never landed.

use crate::frb_generated::StreamSink;
use flutter_rust_bridge::frb;
use soshal_sync_core::engine::{spawn_engine, SyncConfig};
use soshal_sync_core::SyncUpdate;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Live stream listener installed by Dart (one global; last wins).
static SINK: Mutex<Option<StreamSink<String>>> = Mutex::new(None);

/// Stop flag for the running engine (cleared on `sync_stop`).
static STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

fn sink_guard() -> std::sync::MutexGuard<'static, Option<StreamSink<String>>> {
    SINK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Subscribe to live sync updates. Each item is a JSON object tagged by
/// `t`: `feed`, `dm`, `reaction`, or `profile`. The `StreamSink` parameter
/// makes this a Rust→Dart stream: Dart subscribes via `syncEvents()`.
pub fn sync_events(sink: StreamSink<String>) {
    *sink_guard() = Some(sink);
}

/// Serialize one engine update into its Dart-facing JSON payload.
fn update_json(update: SyncUpdate) -> Option<String> {
    match update {
        SyncUpdate::Feed {
            id,
            pubkey,
            content,
            created_at,
            kind,
        } => Some(
            serde_json::json!({
                "t": "feed", "id": id, "pubkey": pubkey,
                "content": content, "created_at": created_at, "kind": kind
            })
            .to_string(),
        ),
        SyncUpdate::Dm {
            id,
            sender,
            content,
            created_at,
        } => {
            // Keys never cross into sync-core; decrypt + persist here.
            let plain = match super::signer::signer_nip44_decrypt(content, sender.clone()) {
                Ok(p) => p,
                Err(_) => return None, // not decryptable by this account → drop
            };
            let recipient = match super::signer::signer_pubkey() {
                Ok(pk) => pk,
                Err(_) => return None,
            };
            // Plaintext goes to `messaging_store_dm`, which is the single
            // sealing point (it AES-GCM-seals at rest). Sealing here too
            // would double-seal (`seal1:seal1:…`), leaving garbage after the
            // single unseal on fetch.
            if super::messaging::messaging_store_dm(
                id.clone(),
                sender.clone(),
                recipient,
                plain.clone(),
                created_at,
                "[]".to_string(),
            )
            .is_err()
            {
                return None;
            }
            Some(
                serde_json::json!({
                    "t": "dm", "id": id, "sender": sender,
                    "content": plain, "created_at": created_at
                })
                .to_string(),
            )
        }
        SyncUpdate::Reaction {
            id,
            event_id,
            pubkey,
            content,
            created_at,
        } => Some(
            serde_json::json!({
                "t": "reaction", "id": id, "event_id": event_id,
                "pubkey": pubkey, "content": content, "created_at": created_at
            })
            .to_string(),
        ),
        SyncUpdate::Profile { pubkey } => {
            Some(serde_json::json!({"t": "profile", "pubkey": pubkey}).to_string())
        }
    }
}

/// Start the background sync engine with the given relay URLs (JSON array).
/// The engine runs on its own OS thread + tokio runtime, so relay
/// connections survive app backgrounding; updates flow to `sync_events`.
#[frb(serialize)]
pub async fn sync_start(relays_json: String) -> Result<String, String> {
    let relays: Vec<String> =
        serde_json::from_str(&relays_json).map_err(|e| format!("invalid relays JSON: {e}"))?;
    if relays.is_empty() {
        return Err("no relay urls".to_string()).into();
    }
    let db_path = super::db::db_path()?;
    let my_pubkey = super::signer::signer_pubkey()?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<SyncUpdate>(256);
    let stop = Arc::new(AtomicBool::new(false));
    spawn_engine(
        SyncConfig {
            db_path,
            my_pubkey,
            relays: relays.clone(),
            socks_proxy: super::network::i2p_socks_addr().map(|a| a.to_string()),
        },
        tx,
        stop.clone(),
    );
    *STOP.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop);

    // Forwarder thread: engine update → JSON → StreamSink. DM decryption
    // happens here (signer lock is per-call, not held across awaits).
    std::thread::spawn(move || {
        while let Some(update) = rx.blocking_recv() {
            let Some(json) = update_json(update) else {
                continue;
            };
            let mut guard = sink_guard();
            match guard.as_mut() {
                Some(sink) => {
                    if sink.add(json).is_err() {
                        *guard = None; // stream closed by Dart
                    }
                }
                None => {}
            }
        }
    });

    Ok(format!("sync engine started ({} relays)", relays.len())).into()
}

/// Stop the background sync engine (disconnects relays; closes its runtime).
#[frb(serialize)]
pub async fn sync_stop() -> Result<bool, String> {
    if let Some(stop) = STOP.lock().unwrap_or_else(|e| e.into_inner()).take() {
        stop.store(true, Ordering::Relaxed);
    }
    Ok(true).into()
}

/// Whether an engine instance is currently running.
#[frb(sync, serialize)]
pub fn sync_running() -> Result<bool, String> {
    let guard = STOP.lock().unwrap_or_else(|e| e.into_inner());
    Ok(guard.is_some()).into()
}

/// Lightweight `"id":"<hex>"` extraction — avoids a full JSON parse just to
/// name the outbox row (the event is parsed once more by the relay send path).
fn event_id_of(event_json: &str) -> Option<String> {
    let marker = "\"id\":\"";
    let start = event_json.find(marker)? + marker.len();
    let end = event_json[start..].find('"')? + start;
    Some(event_json[start..end].to_string())
}

/// Publish a freshly-signed event, or queue it in the persistent outbox when
/// the relay client is unavailable (offline mode). Returns Ok on success or
/// successful enqueue; the outbox is drained by the sync engine.
pub(crate) async fn publish_or_enqueue(action_type: &str, event_json: &str) -> Result<(), String> {
    let event_id = event_id_of(event_json).unwrap_or_else(|| "unknown".to_string());
    match super::network::network_publish_event(event_json.to_string()).await {
        Ok(_) => Ok(()),
        Err(pub_err) => {
            let now = soshal_common_core::format::now_secs();
            let queued = super::db::with_db_result(|db| {
                soshal_sync_core::outbox::enqueue_outbox_item(
                    db,
                    &event_id,
                    action_type,
                    event_json,
                    None,
                    now,
                )
                .map_err(soshal_db_core::error::DbError::Migration)
            });
            match queued {
                Ok(()) => Ok(()),
                Err(e) => Err(format!("publish failed and queue failed: {pub_err}; {e}")),
            }
        }
    }
}

/// Enqueue an action (post, comment, media upload) to the persistent offline outbox queue.
#[frb(sync, serialize)]
pub fn sync_enqueue_outbox(
    action_type: String,
    payload_json: String,
    media_path: Option<String>,
) -> Result<String, String> {
    let id = format!("{:x}", rand::random::<u64>());
    let now = soshal_common_core::format::now_secs();
    super::db::with_db_result(|db| {
        soshal_sync_core::outbox::enqueue_outbox_item(
            db,
            &id,
            &action_type,
            &payload_json,
            media_path.as_deref(),
            now,
        )
        .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(id)
    })
}

/// Get outbox summary counters (pending, failed, total).
#[frb(sync, serialize)]
pub fn sync_get_outbox_summary() -> Result<String, String> {
    super::db::with_db_result(|db| {
        let summary = soshal_sync_core::outbox::get_outbox_summary(db)
            .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(summary)
    })
    .map(super::util::json_ok)?
}

/// Triggers CRDT epoch garbage collection across active peer vector clocks to prune tombstones.
#[frb(sync, serialize)]
pub fn sync_run_epoch_garbage_collection(
    domain: String,
    peer_vector_clocks_json: String,
    gc_threshold_secs: u64,
) -> Result<String, String> {
    let clocks: std::collections::HashMap<String, u64> =
        serde_json::from_str(&peer_vector_clocks_json).unwrap_or_default();
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let summary = soshal_sync_core::epoch_gc::EpochGarbageCollector::prune_tombstones_if_consensus_reached(
            &conn,
            &domain,
            &clocks,
            gc_threshold_secs,
        )
        .map_err(|e| soshal_db_core::error::DbError::Migration(e))?;
        Ok(summary)
    })
    .map(super::util::json_ok)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_json_shapes() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let feed = update_json(SyncUpdate::Feed {
            id: "a".into(),
            pubkey: "b".into(),
            content: "hi".into(),
            created_at: 1,
            kind: 1,
        })
        .unwrap();
        assert!(feed.contains("\"t\":\"feed\""));
        let reaction = update_json(SyncUpdate::Reaction {
            id: "r".into(),
            event_id: "e".into(),
            pubkey: "p".into(),
            content: "+".into(),
            created_at: 2,
        })
        .unwrap();
        assert!(reaction.contains("\"t\":\"reaction\""));
        let profile = update_json(SyncUpdate::Profile { pubkey: "p".into() }).unwrap();
        assert!(profile.contains("\"t\":\"profile\""));
        // DM without an unlocked signer is dropped, not emitted.
        super::super::signer::signer_lock().unwrap();
        let dm = update_json(SyncUpdate::Dm {
            id: "d".into(),
            sender: "p".into(),
            content: "enc".into(),
            created_at: 3,
        });
        assert!(dm.is_none());
    }
}
