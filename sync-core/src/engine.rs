//! Engine lifecycle: a dedicated thread with its own multi-thread tokio
//! runtime owns the relay WebSocket connections, so connection stability is
//! independent of the Flutter isolate / app foreground state. nostr-sdk
//! handles reconnect + backpressure; this loop only verifies, caches, and
//! forwards events.

use crate::ingest::{self, watermark_key};
use crate::{SyncUpdate, WM_DM, WM_FEED, WM_META};
use nostr::filter::Filter;
use nostr::key::PublicKey;
use nostr::types::Timestamp;
use nostr_sdk::client::Client;
use nostr_sdk::prelude::*;
use soshal_db_core::Database;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How far back (secs) the initial/restart watermark reaches, to absorb
/// events that landed between shutdown and reconnect. Watermarks flush every
/// `FLUSH_INTERVAL`, so a feed overlap of a few flushes is ample; DMs are
/// sparse and slow, so keep their overlap generous.
const OVERLAP_FEED_SECS: u64 = 120;
const OVERLAP_DM_SECS: u64 = 300;
const OVERLAP_META_SECS: u64 = 3600;

/// Flush watermarks to SQLite at most this often.
const FLUSH_INTERVAL: Duration = Duration::from_secs(30);

/// Poll cadence while idle, so `stop` is honored promptly.
const IDLE_POLL: Duration = Duration::from_millis(500);

/// After this many consecutive idle polls, the relay stream poll backs off
/// to `IDLE_POLL_BACKOFF` (a closed stream still pings periodically).
const IDLE_BACKOFF_AFTER: u32 = 12;
const IDLE_POLL_BACKOFF: Duration = Duration::from_secs(3);

pub struct SyncConfig {
    pub db_path: String,
    pub my_pubkey: String,
    pub relays: Vec<String>,
    /// SOCKS5 proxy (`ip:port`) for relay connections when i2p is active.
    pub socks_proxy: Option<String>,
}

/// Spawn the background engine on its own thread + runtime. `stop` flips to
/// end the loop; the racing tokio runtime is dropped when the loop exits.
pub fn spawn_engine(
    cfg: SyncConfig,
    tx: tokio::sync::mpsc::Sender<SyncUpdate>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    use rustls::crypto::ring;
    let _ = ring::default_provider().install_default();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("soshal-sync")
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("sync engine: runtime init failed: {e}");
                return;
            }
        };
        rt.block_on(async move {
            if let Err(e) = engine_loop(cfg, tx, stop).await {
                eprintln!("sync engine: {e}");
            }
        });
    })
}

/// Replay pending outbox items over the live relay connections: signed event
/// JSON is parsed and sent; success marks `completed`, failure applies
/// exponential backoff via `mark_outbox_item_failed`.
async fn replay_outbox(db: &Database, client: &Client) {
    let now = soshal_common_core::format::now_secs();
    let items = match crate::outbox::fetch_pending_outbox_items(db, now, 32) {
        Ok(items) => items,
        Err(e) => {
            eprintln!("sync engine: outbox fetch: {e}");
            return;
        }
    };
    let mut completed: Vec<String> = Vec::new();
    for item in items {
        if item.media_path.is_some() {
            continue; // media uploads are handled by the dedicated uploader
        }
        match serde_json::from_str::<Event>(&item.payload_json) {
            Ok(event) => match client.send_event(&event).await {
                Ok(_) => completed.push(item.id.clone()),
                Err(e) => {
                    eprintln!("sync engine: outbox send {}: {e}", item.id);
                    let _ =
                        crate::outbox::mark_outbox_item_failed(db, &item.id, item.retry_count, now);
                }
            },
            Err(e) => {
                eprintln!("sync engine: outbox parse {}: {e}", item.id);
                let _ = crate::outbox::mark_outbox_item_failed(db, &item.id, item.retry_count, now);
            }
        }
    }
    if !completed.is_empty() {
        let _ = crate::outbox::mark_outbox_items_completed(db, &completed);
    }
}

fn ingest_batch(
    db: &Database,
    my_pubkey: &str,
    batch: &[Event],
    tx: &tokio::sync::mpsc::Sender<SyncUpdate>,
    cursors: &mut HashMap<&'static str, u64>,
) {
    match ingest::handle_batch(db, my_pubkey, batch, tx) {
        Ok(()) => {
            for event in batch {
                if let Some(key) = watermark_key(event.kind) {
                    let cur = cursors.entry(key).or_insert(0);
                    let created = event.created_at.as_secs();
                    if created > *cur {
                        *cur = created;
                    }
                }
            }
        }
        Err(e) => eprintln!("sync engine: ingest: {e}"),
    }
}

/// Build the relay client from config (validated URLs + optional SOCKS proxy).
pub async fn build_client(cfg: &SyncConfig) -> Result<Client, String> {
    let mut relays = Vec::new();
    for url in &cfg.relays {
        let (valid, _) = soshal_common_core::url::is_valid_relay_url(url);
        if !valid {
            continue; // SSRF guard: private/loopback/odd schemes rejected
        }
        if let Ok(target) = nostr::types::RelayUrl::parse(url) {
            relays.push(target);
        }
    }
    if relays.is_empty() {
        return Err("no usable relay urls".to_string());
    }
    let mut builder = Client::builder();
    if let Some(proxy) = &cfg.socks_proxy {
        let addr: std::net::SocketAddr = proxy
            .parse()
            .map_err(|e| format!("invalid socks proxy {proxy}: {e}"))?;
        builder = builder.proxy(nostr_sdk::proxy::Proxy::all(addr));
    }
    let client = builder.build();
    for target in &relays {
        client
            .add_relay(target.clone())
            .await
            .map_err(|e| format!("add relay {target}: {e}"))?;
    }
    let _ = client.connect().await;
    Ok(client)
}

async fn engine_loop(
    cfg: SyncConfig,
    tx: tokio::sync::mpsc::Sender<SyncUpdate>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let client = build_client(&cfg).await?;
    engine_loop_with_client(cfg, tx, stop, client).await
}

/// The engine pass itself, against an existing relay client. Split out so
/// short-lived callers (headless background passes) can reuse a warm client
/// and runtime instead of paying the connect handshake each run.
pub async fn engine_loop_with_client(
    cfg: SyncConfig,
    tx: tokio::sync::mpsc::Sender<SyncUpdate>,
    stop: Arc<AtomicBool>,
    client: Client,
) -> Result<(), String> {
    let db = Database::open(&cfg.db_path).map_err(|e| format!("open db: {e}"))?;

    // Subscribe BEFORE polling so nothing negotiated during setup is missed.
    let mut stream = client.notifications();
    let mut cursors: HashMap<&'static str, u64> = HashMap::new();
    cursors.insert(WM_FEED, ingest::watermark(&db, WM_FEED));
    cursors.insert(WM_DM, ingest::watermark(&db, WM_DM));
    cursors.insert(WM_META, ingest::watermark(&db, WM_META));

    if let Ok(pk) = PublicKey::from_hex(&cfg.my_pubkey) {
        let since_feed = Timestamp::from(cursors[WM_FEED].saturating_sub(OVERLAP_FEED_SECS));
        let since_dm = Timestamp::from(cursors[WM_DM].saturating_sub(OVERLAP_DM_SECS));
        let since_meta = Timestamp::from(cursors[WM_META].saturating_sub(OVERLAP_META_SECS));
        let filters = vec![
            Filter::new().kinds([Kind::TextNote]).since(since_feed),
            Filter::new()
                .kinds([Kind::EncryptedDirectMessage])
                .since(since_dm),
            Filter::new().kinds([Kind::Metadata]).since(since_meta),
            // Lists + zaps: contacts (3), relay list (10002), bookmarks
            // (10003), zap receipts (9735).
            Filter::new()
                .kinds([
                    Kind::ContactList,
                    Kind::RelayList,
                    Kind::Bookmarks,
                    Kind::ZapReceipt,
                ])
                .since(since_meta),
            // Minis (31020): mini-app registry URLs land in `posts`.
            Filter::new()
                .kinds([Kind::from(soshal_common_core::consts::KIND_MINIS)])
                .since(since_meta),
            // Self-sync: re-import our own reaction/post events + caches.
            Filter::new()
                .kinds([Kind::Reaction])
                .authors([pk])
                .since(since_feed),
            Filter::new().authors([pk]).since(since_feed),
        ];
        client
            .subscribe(filters)
            .await
            .map_err(|e| format!("subscribe: {e}"))?;
    } else {
        return Err("invalid my_pubkey".to_string());
    }

    // Drain anything queued while the engine was down.
    replay_outbox(&db, &client).await;

    let mut last_flush = Instant::now();
    let mut idle_ticks: u32 = 0;

    let mut batch: Vec<Event> = Vec::with_capacity(64);

    while !stop.load(Ordering::Relaxed) {
        let mut idle = false;
        let poll = if idle_ticks >= IDLE_BACKOFF_AFTER {
            IDLE_POLL_BACKOFF
        } else {
            IDLE_POLL
        };
        match tokio::time::timeout(poll, stream.next()).await {
            Ok(Some(ClientNotification::Event { event, .. })) => {
                batch.push(*event);
            }
            Ok(Some(ClientNotification::Message { .. }))
            | Ok(Some(ClientNotification::Shutdown)) => {}
            Ok(None) => break,
            Err(_) => {
                idle = true;
                idle_ticks = idle_ticks.saturating_add(1);
            }
        }
        if !idle {
            idle_ticks = 0;
        }

        if (idle && !batch.is_empty()) || batch.len() >= 64 {
            ingest_batch(&db, &cfg.my_pubkey, &batch, &tx, &mut cursors);
            batch.clear();
        }

        if last_flush.elapsed() >= FLUSH_INTERVAL {
            for (key, ts) in &cursors {
                ingest::set_watermark(&db, key, *ts);
            }
            replay_outbox(&db, &client).await;
            last_flush = Instant::now();
        }
    }

    if !batch.is_empty() {
        ingest_batch(&db, &cfg.my_pubkey, &batch, &tx, &mut cursors);
    }
    for (key, ts) in &cursors {
        ingest::set_watermark(&db, key, *ts);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watermark_cursors_seed_parallel() {
        // Cursor map mirrors the keys the engine subscribes on.
        let mut cursors: HashMap<&'static str, u64> = HashMap::new();
        cursors.insert(WM_FEED, 0);
        cursors.insert(WM_DM, 0);
        cursors.insert(WM_META, 0);
        assert_eq!(cursors.len(), 3);
    }
}
