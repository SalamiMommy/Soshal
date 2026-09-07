//! Headless background sync module for Android WorkManager and iOS BGTaskScheduler.
//!
//! Executes directly from the background task runner without spinning up
//! the Flutter Engine or Dart VM.
//!
//! WorkManager may fire repeated passes in the same process; each pass
//! reuses the previous pass's tokio runtime and relay client (when the relay
//! list is unchanged), so the dominant per-pass cost — runtime spawn +
//! WebSocket connect handshake — is paid once, not per pass.

use soshal_sync_core::engine::{build_client, engine_loop_with_client_sealed, SyncConfig};
use soshal_sync_core::SyncUpdate;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How long (secs) a headless sync pass keeps the relay engine alive.
const SYNC_PASS_SECS: u64 = 20;

struct HeadlessCtx {
    /// Joined relay list this client was built for; a changed list invalidates.
    relays_key: String,
    client: nostr_sdk::client::Client,
}

static HEADLESS_CTX: Mutex<Option<HeadlessCtx>> = Mutex::new(None);

/// Run a bounded one-shot sync pass against the relay network for the active
/// account. Async: the pass may take up to SYNC_PASS_SECS and must not block
/// the Dart isolate.
#[flutter_rust_bridge::frb(serialize)]
pub async fn background_sync_task(db_path: String) -> Result<i32, String> {
    if db_path.is_empty() {
        return Err("Database path cannot be empty".to_string());
    }

    let db = match soshal_db_core::Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => return Err(format!("Failed to open DB for background sync: {e}")).into(),
    };

    if let Err(e) = db.migrate() {
        return Err(format!("Failed to migrate DB during background sync: {e}")).into();
    }

    // Load the active account + relay list from session.json (next to the
    // DB file). Headless runs may execute in a separate process, so the
    // in-memory SESSION static is not trusted here.
    let session_path = Path::new(&db_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("session.json");
    let (pubkey, relays) = match std::fs::read_to_string(&session_path)
        .ok()
        .and_then(|c| serde_json::from_str::<super::session::SessionData>(&c).ok())
    {
        Some(session) => {
            let active = session.active_pubkey.clone().unwrap_or_default();
            let relays = session
                .accounts
                .iter()
                .find(|a| a.pubkey == active)
                .map(|a| a.relay_list.clone())
                .unwrap_or_default();
            (active, relays)
        }
        None => (String::new(), Vec::new()),
    };

    if pubkey.is_empty() || relays.is_empty() {
        return Ok(0); // no configured account/relays: nothing to sync
    }

    let relays_key = relays.join(",");
    let (stale, cached_client) = {
        let guard = crate::ffi::util::lock(&HEADLESS_CTX);
        match guard.as_ref() {
            Some(ctx) if ctx.relays_key == relays_key => (false, Some(ctx.client.clone())),
            _ => (true, None),
        }
    };
    let client = if stale {
        let cfg = SyncConfig {
            db_path: db_path.clone(),
            my_pubkey: pubkey.clone(),
            relays: relays.clone(),
            socks_proxy: None,
        };
        let client = build_client(&cfg)
            .await
            .map_err(|e| format!("Failed to build relay client: {e}"))?;
        let mut guard = crate::ffi::util::lock(&HEADLESS_CTX);
        *guard = Some(HeadlessCtx {
            relays_key,
            client: client.clone(),
        });
        client
    } else {
        cached_client.ok_or_else(|| "headless ctx missing".to_string())?
    };

    // Bounded one-shot pass against the warm client: run the engine, then
    // stop after SYNC_PASS_SECS. Events are ingested + outbox items replayed
    // by the engine loop.
    let (tx, _rx) = tokio::sync::mpsc::channel::<SyncUpdate>(16);
    let stop = Arc::new(AtomicBool::new(false));
    let cfg = SyncConfig {
        db_path,
        my_pubkey: pubkey,
        relays,
        socks_proxy: None,
    };
    let unseal = super::sync::outbox_unseal_fn();
    let _ = client.connect().await; // no-op when already connected
    let handle = tokio::spawn(engine_loop_with_client_sealed(
        cfg,
        tx,
        stop.clone(),
        client,
        unseal,
    ));
    tokio::time::sleep(std::time::Duration::from_secs(SYNC_PASS_SECS)).await;
    stop.store(true, Ordering::Relaxed);
    let _ = handle.await;
    Ok(1).into()
}
