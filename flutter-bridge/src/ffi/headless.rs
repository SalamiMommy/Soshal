//! Headless background sync module for Android WorkManager and iOS BGTaskScheduler.
//!
//! Executes directly from the background task runner without spinning up
//! the Flutter Engine or Dart VM.

use soshal_sync_core::engine::{spawn_engine, SyncConfig};
use soshal_sync_core::SyncUpdate;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// How long (secs) a headless sync pass keeps the relay engine alive.
const SYNC_PASS_SECS: u64 = 20;

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

    // Bounded one-shot pass: run the engine, then stop after SYNC_PASS_SECS.
    // Events are ingested + outbox items replayed by engine_loop.
    let (tx, _rx) = tokio::sync::mpsc::channel::<SyncUpdate>(16);
    let stop = Arc::new(AtomicBool::new(false));
    let handle = spawn_engine(
        SyncConfig {
            db_path,
            my_pubkey: pubkey,
            relays,
            socks_proxy: None,
        },
        tx,
        stop.clone(),
    );
    std::thread::sleep(std::time::Duration::from_secs(SYNC_PASS_SECS));
    stop.store(true, Ordering::Relaxed);
    let _ = handle.join();
    Ok(1)
}
