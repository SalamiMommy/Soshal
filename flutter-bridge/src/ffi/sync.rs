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
use soshal_sync_core::engine::{spawn_engine_sealed, SyncConfig};
use soshal_sync_core::SyncUpdate;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Live stream listener installed by Dart (one global; last wins).
static SINK: Mutex<Option<StreamSink<String>>> = Mutex::new(None);

/// Stop flag for the running engine (cleared on `sync_stop`).
static STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

fn sink_guard() -> std::sync::MutexGuard<'static, Option<StreamSink<String>>> {
    crate::ffi::util::lock(&SINK)
}

/// Push a serialized update to the Dart stream (used by the mesh ingest
/// task, mirroring the sync engine's forwarder).
pub(crate) fn push_update_to_sink(json: String) {
    let mut guard = sink_guard();
    if let Some(sink) = guard.as_mut() {
        if sink.add(json).is_err() {
            *guard = None;
        }
    }
}

/// Subscribe to live sync updates. Each item is a JSON object tagged by
/// `t`: `feed`, `dm`, `reaction`, or `profile`. The `StreamSink` parameter
/// makes this a Rust→Dart stream: Dart subscribes via `syncEvents()`.
pub fn sync_events(sink: StreamSink<String>) {
    *sink_guard() = Some(sink);
}

#[derive(serde::Serialize)]
struct FeedSyncDto<'a> {
    t: &'static str,
    id: &'a str,
    pubkey: &'a str,
    content: &'a str,
    created_at: u64,
    kind: u64,
}

#[derive(serde::Serialize)]
struct ReactionSyncDto<'a> {
    t: &'static str,
    id: &'a str,
    event_id: &'a str,
    pubkey: &'a str,
    content: &'a str,
    created_at: u64,
}

#[derive(serde::Serialize)]
struct ProfileSyncDto<'a> {
    t: &'static str,
    pubkey: &'a str,
}

/// Serialize one engine update into its Dart-facing JSON payload.
pub(crate) fn update_json(update: SyncUpdate) -> Option<String> {
    match update {
        SyncUpdate::Feed {
            id,
            pubkey,
            content,
            created_at,
            kind,
        } => serde_json::to_string(&FeedSyncDto {
            t: "feed",
            id: &id,
            pubkey: &pubkey,
            content: &content,
            created_at,
            kind,
        })
        .ok(),
        SyncUpdate::Dm {
            id,
            sender,
            recipient,
            content,
            created_at,
            tags_json,
        } => {
            let my_pk = match super::signer::signer_pubkey() {
                Ok(pk) => pk.trim().to_ascii_lowercase(),
                Err(_) => return None,
            };
            let sender_clean = sender.trim().to_ascii_lowercase();
            let recipient_clean = recipient.trim().to_ascii_lowercase();
            if super::db::with_db_result(|db| {
                soshal_db_core::repos::block::BlockRepo::new(db).is_blocked(&my_pk, &sender_clean)
            })
            .unwrap_or(false)
            {
                return None;
            }
            let peer = if sender_clean == my_pk {
                recipient_clean.clone()
            } else {
                sender_clean.clone()
            };
            // Keys never cross into sync-core; decrypt + persist here.
            let plain = match super::signer::signer_nip44_decrypt(content, peer) {
                Ok(p) => p,
                Err(_) => return None, // not decryptable by this account → drop
            };
            // Plaintext goes to `messaging_store_dm`, which is the single
            // sealing point (it AES-GCM-seals at rest). Sealing here too
            // would double-seal (`seal1:seal1:…`), leaving garbage after the
            // single unseal on fetch.
            if super::messaging::messaging_store_dm(
                id.clone(),
                sender_clean.clone(),
                recipient_clean.clone(),
                plain.to_string(),
                created_at,
                tags_json.clone(),
            )
            .is_err()
            {
                return None;
            }
            let tags = serde_json::from_str::<serde_json::Value>(&tags_json)
                .unwrap_or(serde_json::Value::Null);
            Some(
                serde_json::json!({
                    "t": "dm", "id": id, "sender": sender_clean,
                    "recipient": recipient_clean, "content": plain, "created_at": created_at,
                    "tags": tags
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
        } => serde_json::to_string(&ReactionSyncDto {
            t: "reaction",
            id: &id,
            event_id: &event_id,
            pubkey: &pubkey,
            content: &content,
            created_at,
        })
        .ok(),
        SyncUpdate::Profile { pubkey } => serde_json::to_string(&ProfileSyncDto {
            t: "profile",
            pubkey: &pubkey,
        })
        .ok(),
    }
}

/// Start the background sync engine with the given relay URLs (JSON array).
/// The engine runs on its own OS thread + tokio runtime, so relay
/// connections survive app backgrounding; updates flow to `sync_events`.
#[frb(serialize)]
pub async fn sync_start(relays_json: String) -> Result<String, String> {
    if relays_json.len() > 64 * 1024 {
        return Err("relays JSON too large (max 64KB)".to_string()).into();
    }
    let relays_raw: Vec<String> =
        serde_json::from_str(&relays_json).map_err(|e| format!("invalid relays JSON: {e}"))?;
    let mut relays = Vec::new();
    for r in relays_raw {
        let trimmed = r.trim();
        if (trimmed.starts_with("ws://") || trimmed.starts_with("wss://")) && trimmed.len() <= 1024
        {
            relays.push(trimmed.to_string());
            if relays.len() >= 50 {
                break;
            }
        }
    }
    if relays.is_empty() {
        return Err("no valid relay urls".to_string()).into();
    }
    let db_path = super::db::db_path()?;
    let my_pubkey = super::signer::signer_pubkey()?;

    // Guard against double-start: an existing engine must be stopped first,
    // otherwise its STOP flag would be overwritten and its thread would run
    // forever (uncontrollable + two engines at once).
    if let Some(prev) = crate::ffi::util::lock(&STOP).take() {
        prev.store(true, Ordering::Relaxed);
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<SyncUpdate>(256);
    let stop = Arc::new(AtomicBool::new(false));

    // Install the stop flag BEFORE spawning: a fast-failing engine thread may
    // exit before `spawn_engine` returns, and its on_exit must find the
    // static populated (or a newer generation) to clear reconcilably.
    {
        let mut guard = crate::ffi::util::lock(&STOP);
        *guard = Some(stop.clone());
    }
    let exit_stop = stop.clone();
    let unseal = outbox_unseal_fn();
    spawn_engine_sealed(
        SyncConfig {
            db_path,
            my_pubkey,
            relays: relays.clone(),
            socks_proxy: super::network::i2p_socks_addr().map(|a| a.to_string()),
        },
        tx,
        stop,
        move || {
            // Engine loop terminated (stop flag, relay stream end, or a
            // startup/subscribe error). Clear the static so `sync_running()`
            // reports the truth and, if it is still this generation's flag, a
            // later `sync_start` is the only writer. A newer Arc in the static
            // means a restart already happened — leave it untouched.
            let mut guard = crate::ffi::util::lock(&STOP);
            if let Some(cur) = guard.as_ref() {
                if Arc::ptr_eq(cur, &exit_stop) {
                    *guard = None;
                }
            }
        },
        unseal,
    );

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
    if let Some(stop) = crate::ffi::util::lock(&STOP).take() {
        stop.store(true, Ordering::Relaxed);
    }
    Ok(true).into()
}

/// Whether an engine instance is currently running.
#[frb(sync, serialize)]
pub fn sync_running() -> Result<bool, String> {
    let guard = crate::ffi::util::lock(&STOP);
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

/// Seal an outbox payload at rest. Compressed payload is base64'd into a
/// `__seal_b64__:` envelope under the signer's at-rest key. Signer-lock /
/// pre-init degrade to storing the payload plaintext (identity) so publishing
/// never hard-fails on an unlocked wallet.
pub(crate) fn outbox_seal_fn() -> impl Fn(String) -> Result<String, String> {
    move |compressed: String| -> Result<String, String> {
        let key = match super::signer::signer_at_rest_key() {
            Ok(k) => k,
            Err(_) => return Ok(compressed),
        };
        let sealed = soshal_crypto_core::at_rest::seal_at_rest_bin(&key, compressed.as_bytes())
            .map_err(|e| format!("seal outbox: {e}"))?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(sealed);
        Ok(format!("{}{b64}", soshal_sync_core::outbox::SEAL_PREFIX))
    }
}

/// Unseal an outbox payload previously written via `outbox_seal_fn`. Receives
/// the base64 body already stripped of the `__seal_b64__:` envelope by
/// `soshal_sync_core::outbox::unseal_payload`.
pub(crate) fn outbox_unseal_fn() -> impl Fn(&str) -> Result<String, String> + Send + Sync {
    move |inner: &str| -> Result<String, String> {
        use base64::Engine;
        let blob = base64::engine::general_purpose::STANDARD
            .decode(inner)
            .map_err(|e| format!("outbox seal b64: {e}"))?;
        let key = super::signer::signer_at_rest_key().map_err(|e| format!("at-rest key: {e}"))?;
        let plain = soshal_crypto_core::at_rest::open_at_rest_bin(&key, &blob)
            .map_err(|e| format!("open outbox: {e}"))?;
        String::from_utf8(plain).map_err(|e| format!("outbox utf8: {e}"))
    }
}

/// Publish a freshly-signed event, or queue it in the persistent outbox when
/// no transport is available (offline mode). The mesh relay node is tried
/// first when it is the resolved transport; otherwise the relay client.
/// Returns Ok on success or successful enqueue; the outbox is drained by
/// the sync engine.
pub(crate) async fn publish_or_enqueue(action_type: &str, event_json: &str) -> Result<(), String> {
    let event_id = event_id_of(event_json).unwrap_or_else(|| "unknown".to_string());
    let mut published = false;
    if let Ok(mesh_ok) = super::relay::try_mesh_publish(event_json) {
        if mesh_ok {
            published = true;
        }
    }
    let result: Result<(), String> = if published {
        Ok(())
    } else {
        super::network::network_publish_event(event_json.to_string())
            .await
            .map(|_| ())
    };
    match result {
        Ok(_) => Ok(()),
        Err(pub_err) => {
            let now = soshal_common_core::format::now_secs();
            let queued = super::db::with_db_result(|db| {
                soshal_sync_core::outbox::enqueue_outbox_item_with_seal(
                    db,
                    &event_id,
                    action_type,
                    event_json,
                    None,
                    now,
                    outbox_seal_fn(),
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
    if payload_json.len() > 16 * 1024 * 1024 {
        return Err("payload JSON exceeds 16MB cap".to_string());
    }
    if action_type.is_empty() || action_type.len() > 128 {
        return Err("action_type must be 1..=128 chars".to_string());
    }
    if let Some(path) = &media_path {
        if path.len() > 4096 {
            return Err("media_path exceeds 4096-char cap".to_string());
        }
    }
    let id = format!("{:x}", rand::random::<u64>());
    let now = soshal_common_core::format::now_secs();
    super::db::with_db_result(|db| {
        soshal_sync_core::outbox::enqueue_outbox_item_with_seal(
            db,
            &id,
            &action_type,
            &payload_json,
            media_path.as_deref(),
            now,
            outbox_seal_fn(),
        )
        .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(id)
    })
}

/// Get outbox summary counters (pending, failed, total).
#[frb(sync, serialize)]
pub fn sync_get_outbox_summary() -> Result<String, String> {
    super::db::with_db_result(|db| {
        let summary = soshal_sync_core::outbox::summarize_outbox(db)
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
    if domain.is_empty() || domain.len() > 128 {
        return Err("domain must be 1..=128 chars".to_string());
    }
    if peer_vector_clocks_json.len() > 1024 * 1024 {
        return Err("peer vector clocks JSON exceeds 1MB cap".to_string());
    }
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
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
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
            recipient: "me".into(),
            content: "enc".into(),
            created_at: 3,
            tags_json: "[[\"p\",\"me\"]]".to_string(),
        });
        assert!(dm.is_none());
    }

    #[test]
    fn test_event_id_of_extraction() {
        assert_eq!(
            event_id_of(r#"{"id":"deadbeef","kind":1}"#).as_deref(),
            Some("deadbeef")
        );
        assert_eq!(event_id_of(r#"{"kind":1}"#), None);
        assert_eq!(event_id_of(r#"{"id":""}"#).as_deref(), Some(""));
        assert_eq!(event_id_of("garbage"), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_or_enqueue_falls_back_to_outbox() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync", "sync");
        // No relay client is configured in tests: publish fails fast, so the
        // item must land in the persistent outbox.
        let event_json = r#"{"id":"sync-outbox-1","pubkey":"a","created_at":1,"kind":1,"tags":[],"content":"hi","sig":"00"}"#;
        publish_or_enqueue("post", event_json).await.unwrap();
        let rows = crate::ffi::db::db_query_raw_test(
            "SELECT id, action_type FROM outbox_queue WHERE id='sync-outbox-1'".to_string(),
        )
        .unwrap();
        assert!(rows.contains("sync-outbox-1"), "rows: {rows}");
        assert!(rows.contains("post"), "rows: {rows}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_or_enqueue_queues_invalid_json() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync-bad", "sync");
        // Even malformed JSON is queued (publish fails, outbox succeeds);
        // the event id falls back to "unknown".
        publish_or_enqueue("post", "not-json").await.unwrap();
        let rows = crate::ffi::db::db_query_raw_test(
            "SELECT id, action_type FROM outbox_queue WHERE id='unknown'".to_string(),
        )
        .unwrap();
        assert!(rows.contains("unknown"), "rows: {rows}");
        assert!(rows.contains("post"), "rows: {rows}");
    }

    #[test]
    fn test_sync_enqueue_outbox_and_summary() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync-enqueue", "sync");
        let id = sync_enqueue_outbox(
            "post".to_string(),
            r#"{"kind":1,"content":"hi"}"#.to_string(),
            None,
        )
        .unwrap();
        assert!(!id.is_empty());
        let v: serde_json::Value =
            serde_json::from_str(&sync_get_outbox_summary().unwrap()).unwrap();
        assert_eq!(v["pending_count"], 1);
        assert_eq!(v["failed_count"], 0);
        assert_eq!(v["total_count"], 1);
        let rows = crate::ffi::db::db_query_params(
            "SELECT id, action_type, status FROM outbox_queue WHERE id=?1",
            &[id.clone()],
        )
        .unwrap();
        assert!(rows.contains("post"), "rows: {rows}");
        assert!(rows.contains("pending"), "rows: {rows}");
    }

    #[test]
    fn test_sync_get_outbox_summary_empty_db() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync-summary-empty", "sync");
        let v: serde_json::Value =
            serde_json::from_str(&sync_get_outbox_summary().unwrap()).unwrap();
        assert_eq!(v["pending_count"], 0);
        assert_eq!(v["failed_count"], 0);
        assert_eq!(v["total_count"], 0);
    }

    #[test]
    fn test_sync_run_epoch_garbage_collection() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync-gc", "sync");
        // No peer clocks → consensus impossible, summary is all zeros.
        let v: serde_json::Value = serde_json::from_str(
            &sync_run_epoch_garbage_collection("feed".to_string(), "{}".to_string(), 3600).unwrap(),
        )
        .unwrap();
        assert_eq!(v["domain"], "feed");
        assert_eq!(v["pruned_tombstones"], 0);
        // Seed a tombstone at created_at=0.
        crate::ffi::db::insert_test_user("pk");
        crate::ffi::db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('gc-tomb','pk','x',1,0,'[]','synced',1)"
                .to_string(),
        )
        .unwrap();
        // Clock floor: a peer horizon at or below the threshold must not
        // collapse the cutoff to 0 and wipe every tombstone.
        let v: serde_json::Value = serde_json::from_str(
            &sync_run_epoch_garbage_collection(
                "feed".to_string(),
                r#"{"peer1":3600}"#.to_string(),
                3600,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(v["epoch_counter"], 0, "floor no-op expected");
        assert_eq!(v["pruned_tombstones"], 0);
        // Consensus horizon above the threshold → cutoff prunes the tombstone.
        let v: serde_json::Value = serde_json::from_str(
            &sync_run_epoch_garbage_collection(
                "feed".to_string(),
                r#"{"peer1":100000}"#.to_string(),
                3600,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(v["epoch_counter"], 1);
        assert_eq!(v["pruned_tombstones"], 1);
        let rows = crate::ffi::db::db_query_raw_test(
            "SELECT id FROM posts WHERE id='gc-tomb'".to_string(),
        )
        .unwrap();
        assert!(!rows.contains("gc-tomb"), "tombstone not pruned: {rows}");
    }

    #[tokio::test]
    async fn test_sync_stop_and_running_no_engine() {
        // No engine is ever started in tests → STOP stays None.
        assert!(sync_stop().await.unwrap());
        assert!(!sync_running().unwrap());
    }

    #[test]
    fn test_update_json_dm_happy_path() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync-dm", "sync");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let pk = keys.public_key().to_hex();
        crate::ffi::db::insert_test_user(&pk);
        // Self-DM: payload encrypted to own pubkey decrypts with own key.
        let payload =
            super::super::signer::signer_nip44_encrypt("hello dm".to_string(), pk.clone()).unwrap();
        let json = update_json(SyncUpdate::Dm {
            id: "dm-1".into(),
            sender: pk.clone(),
            recipient: pk,
            content: payload,
            created_at: 1234,
            tags_json: "[[\"p\",\"me\"],[\"e\",\"parent\",\"\",\"reply\"]]".to_string(),
        })
        .expect("decryptable DM must emit JSON and persist");
        assert!(json.contains("\"t\":\"dm\""), "json: {json}");
        assert!(json.contains("hello dm"), "json: {json}");
        let rows = crate::ffi::db::db_query_raw_test(
            "SELECT id, pubkey FROM messages WHERE id='dm-1'".to_string(),
        )
        .unwrap();
        assert!(rows.contains("dm-1"), "rows: {rows}");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_update_json_dm_case_insensitive_and_blocked() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = crate::ffi::db::tmp_db("sync-dm-case", "sync");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let pk = keys.public_key().to_hex();
        crate::ffi::db::insert_test_user(&pk);

        // Self-DM with uppercase sender hex
        let payload =
            super::super::signer::signer_nip44_encrypt("case dm".to_string(), pk.clone()).unwrap();
        let json = update_json(SyncUpdate::Dm {
            id: "dm-case-1".into(),
            sender: pk.to_uppercase(),
            recipient: pk.clone(),
            content: payload,
            created_at: 1234,
            tags_json: "[]".to_string(),
        })
        .expect("decryptable DM with uppercase sender must emit JSON");
        assert!(json.contains("case dm"));

        // Blocked sender with casing mismatch
        let blocked = "b".repeat(64);
        crate::ffi::db::insert_test_user(&blocked);
        crate::ffi::db::db_execute_raw_test(format!(
            "INSERT INTO blocks (pubkey, blocked_pubkey, created_at) VALUES ('{}', '{}', 12345)",
            pk.to_lowercase(),
            blocked.to_lowercase(),
        ))
        .unwrap();

        let blocked_dm = update_json(SyncUpdate::Dm {
            id: "dm-blocked".into(),
            sender: blocked.to_uppercase(),
            recipient: pk,
            content: "ignored".into(),
            created_at: 1234,
            tags_json: "[]".to_string(),
        });
        assert!(blocked_dm.is_none(), "blocked sender DM must be dropped");

        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test]
    async fn test_sync_start_and_outbox_bounds() {
        let huge_relays = format!("[\"{}\"]", "ws://".to_string() + &"r".repeat(65 * 1024));
        assert!(sync_start(huge_relays).await.is_err());

        let invalid_relays =
            serde_json::to_string(&vec!["http://relay.damus.io", "ftp://foo"]).unwrap();
        assert!(sync_start(invalid_relays).await.is_err());

        let huge_outbox = "x".repeat(16 * 1024 * 1024 + 10);
        assert!(sync_enqueue_outbox("post".to_string(), huge_outbox, None).is_err());
        assert!(sync_enqueue_outbox(String::new(), "{}".to_string(), None).is_err());

        let huge_domain = "d".repeat(129);
        assert!(sync_run_epoch_garbage_collection(huge_domain, "{}".to_string(), 100).is_err());
    }
}
