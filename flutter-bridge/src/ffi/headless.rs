//! Headless background sync module for Android WorkManager and iOS BGTaskScheduler.
//!
//! Executes directly from the background task runner without spinning up
//! the Flutter Engine or Dart VM.
//!
//! WorkManager may fire repeated passes in the same process; each pass
//! reuses the previous pass's tokio runtime and relay client (when the relay
//! list is unchanged), so the dominant per-pass cost — runtime spawn +
//! WebSocket connect handshake — is paid once, not per pass.

use soshal_sync_core::engine::{build_client, engine_loop_with_client, SyncConfig};
use soshal_sync_core::SyncUpdate;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How long (secs) a headless sync pass keeps the relay engine alive.
const SYNC_PASS_SECS: u64 = 20;

struct HeadlessCtx {
    rt: tokio::runtime::Runtime,
    /// Joined relay list this client was built for; a changed list invalidates.
    relays_key: String,
    client: nostr_sdk::client::Client,
}

static HEADLESS_CTX: Mutex<Option<HeadlessCtx>> = Mutex::new(None);

#[flutter_rust_bridge::frb(sync, serialize)]
pub fn background_sync_task(db_path: String) -> Result<i32, String> {
    if db_path.is_empty() {
        return Err("Database path cannot be empty".to_string());
    }

    let db = soshal_db_core::Database::open(&db_path)
        .map_err(|e| format!("Failed to open DB for background sync: {}", e))?;

    db.migrate()
        .map_err(|e| format!("Failed to migrate DB during background sync: {}", e))?;

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
    let mut guard = HEADLESS_CTX.lock().unwrap_or_else(|e| e.into_inner());
    let stale = match guard.as_ref() {
        Some(ctx) => ctx.relays_key != relays_key,
        None => true,
    };
    if stale {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("soshal-headless")
            .build()
            .map_err(|e| format!("Failed to build headless runtime: {e}"))?;
        let cfg = SyncConfig {
            db_path: db_path.clone(),
            my_pubkey: pubkey.clone(),
            relays: relays.clone(),
            socks_proxy: None,
        };
        let client = rt
            .block_on(build_client(&cfg))
            .map_err(|e| format!("Failed to build relay client: {e}"))?;
        *guard = Some(HeadlessCtx {
            rt,
            relays_key,
            client,
        });
    }
    let ctx = guard.as_ref().expect("ctx initialized above");

    // Bounded one-shot pass against the warm client: run the engine, then
    // stop after SYNC_PASS_SECS. Events are ingested + outbox items replayed
    // by the engine loop.
    let (tx, _rx) = tokio::sync::mpsc::channel::<SyncUpdate>(16);
    let stop = Arc::new(AtomicBool::new(false));
    let client = ctx.client.clone();
    let cfg = SyncConfig {
        db_path,
        my_pubkey: pubkey,
        relays,
        socks_proxy: None,
    };
    ctx.rt.block_on(async {
        let _ = client.connect().await; // no-op when already connected
        let handle = tokio::spawn(engine_loop_with_client(cfg, tx, stop.clone(), client));
        tokio::time::sleep(std::time::Duration::from_secs(SYNC_PASS_SECS)).await;
        stop.store(true, Ordering::Relaxed);
        let _ = handle.await;
    });
    Ok(1)
}
