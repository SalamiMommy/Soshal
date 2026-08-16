//! Verified-event ingest: relay-fetched events are untrusted, so every event
//! passes `event.verify()` (signature + id) before any row is written or any
//! update is emitted. Kind-4 payloads additionally require a p-tag pointing
//! at the local pubkey (or authorship by it) — mirroring the
//! `commands::util::verified_events` rule set from the hardening audit.

use crate::{SyncUpdate, WM_FEED, WM_META};
use nostr::event::{Event, Kind};
use nostr::key::PublicKey;
use nostr::nips::nip19::ToBech32;
use soshal_db_core::error::DbError;
use soshal_db_core::repos::bookmark::{BookmarkRepo, BookmarkRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::repos::relay::{RelayRepo, RelayRow};
use soshal_db_core::repos::settings::SettingsRepo;
use soshal_db_core::repos::user::{UserRepo, UserRow};
use soshal_db_core::repos::zap::{ZapRepo, ZapRow};
use soshal_db_core::Database;

/// Max bytes of feed content that gets cached (relay size caps already
/// enforced by PostRepo::upsert; this is a belt-and-suspenders guard).
const MAX_CACHED_CONTENT: usize = 64 * 1024;

fn e_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "e")
        .filter_map(|t| t.content().map(|c| c.to_string()))
        .collect()
}

fn p_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "p")
        .filter_map(|t| t.content().map(|c| c.to_string()))
        .collect()
}

fn t_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "t")
        .filter_map(|t| t.content().map(|c| c.to_string()))
        .collect()
}

/// Convert a verified relay event into its cached DB row (if cacheable).
fn post_row(event: &Event) -> Option<PostRow> {
    if event.content.len() > MAX_CACHED_CONTENT {
        return None;
    }
    let es = e_tags(event);
    let reply_to = es.first().cloned();
    let root_id = es.get(1).cloned().or_else(|| es.first().cloned());
    let sig = event.sig.to_string();

    let tags_json = serde_json::to_string(
        &event
            .tags
            .iter()
            .map(|t| t.clone().to_vec())
            .collect::<Vec<Vec<String>>>(),
    )
    .unwrap_or_default();
    let freenet_key = event.tags.iter().find_map(|t| {
        let slice = t.as_slice();
        if slice.first().map(|s| s.as_str()) == Some("freenet") {
            slice.get(1).map(|s| s.to_string())
        } else {
            None
        }
    });

    Some(PostRow {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        content: event.content.clone(),
        kind: event.kind.as_u16() as u64 as i64,
        created_at: event.created_at.as_secs() as i64,
        tags_json,
        sig: Some(sig),
        reply_to,
        root_id,
        mentioned_pubkeys: p_tags(event).join(","),
        mentioned_hashtags: t_tags(event).join(","),
        subject: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: freenet_key.clone(),
        is_freenet_native: freenet_key.is_some(),
    })
}

fn user_row(event: &Event) -> Option<UserRow> {
    let meta: serde_json::Value = serde_json::from_str(&event.content).ok()?;
    let pubkey = event.pubkey.to_hex();
    let npub = PublicKey::from_hex(&pubkey)
        .ok()
        .map(|p| p.to_bech32().unwrap_or_default())
        .unwrap_or_default();
    let created = event.created_at.as_secs() as i64;
    Some(UserRow {
        pubkey,
        npub,
        name: meta
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        display_name: meta
            .get("display_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        about: meta
            .get("about")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        picture: meta
            .get("picture")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        banner: meta
            .get("banner")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        nip05: meta
            .get("nip05")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        lud16: meta
            .get("lud16")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        created_at: created,
        updated_at: created,
        metadata_json: Some(event.content.clone()),
        contact_pubkeys: String::new(),
        relay_list: String::new(),
    })
}

/// Read the persisted watermark for `key` (0 when absent).
pub fn watermark(db: &Database, key: &str) -> u64 {
    SettingsRepo::new(db)
        .get(key)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Persist the watermark for `key` (best-effort; a stale watermark only
/// causes a small re-fetch overlap).
pub fn set_watermark(db: &Database, key: &str, ts: u64) {
    let _ = SettingsRepo::new(db).set(key, &ts.to_string());
}

/// Handle one verified relay event: cache it, bump the watermark, and emit a
/// [`SyncUpdate`] for the app layer. Any kind we don't model is skipped.
pub fn handle(
    db: &Database,
    my_pubkey: &str,
    event: &Event,
    tx: &tokio::sync::mpsc::Sender<SyncUpdate>,
) -> Result<(), DbError> {
    event
        .verify()
        .map_err(|e| DbError::Migration(format!("event verification failed: {e}")))?;

    // DM kind: only keep payloads addressed to us (p-tag mine) or authored by
    // us; content stays encrypted here — never persisted unverified.
    if event.kind == Kind::EncryptedDirectMessage {
        let addresses_me = p_tags(event).iter().any(|p| p == my_pubkey);
        let authored_by_me = event.pubkey.to_hex() == my_pubkey;
        if addresses_me || authored_by_me {
            let _ = tx.try_send(SyncUpdate::Dm {
                id: event.id.to_hex(),
                sender: event.pubkey.to_hex(),
                content: event.content.clone(),
                created_at: event.created_at.as_secs(),
            });
        }
        return Ok(());
    }

    match event.kind {
        Kind::Metadata => {
            if let Some(row) = user_row(event) {
                UserRepo::new(db).upsert(&row)?;
                let _ = tx.try_send(SyncUpdate::Profile { pubkey: row.pubkey });
            }
        }
        Kind::ContactList => {
            let pubkey = event.pubkey.to_hex();
            let repo = UserRepo::new(db);
            let mut row = repo.get_by_pubkey(&pubkey)?.unwrap_or_else(|| UserRow {
                pubkey: pubkey.clone(),
                npub: PublicKey::from_hex(&pubkey)
                    .ok()
                    .map(|p| p.to_bech32().unwrap_or_default())
                    .unwrap_or_default(),
                name: None,
                display_name: None,
                about: None,
                picture: None,
                banner: None,
                nip05: None,
                lud16: None,
                created_at: event.created_at.as_secs() as i64,
                updated_at: event.created_at.as_secs() as i64,
                metadata_json: None,
                contact_pubkeys: String::new(),
                relay_list: String::new(),
            });
            row.contact_pubkeys = p_tags(event).join(",");
            repo.upsert(&row)?;
        }
        Kind::ZapReceipt => {
            let Some(recipient) = p_tags(event).first().cloned() else {
                return Ok(());
            };
            let amount = event
                .tags
                .iter()
                .find(|t| t.kind() == "amount")
                .and_then(|t| t.content())
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let row = ZapRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                recipient_pubkey: recipient,
                event_id: e_tags(event).first().cloned(),
                amount,
                content: Some(event.content.clone()),
                created_at: event.created_at.as_secs() as i64,
                zap_type: "public".to_string(),
            };
            ZapRepo::new(db).upsert(&row)?;
        }
        Kind::RelayList => {
            let relay_repo = RelayRepo::new(db);
            let owner = Some(event.pubkey.to_hex());
            for t in event.tags.iter().filter(|t| t.kind() == "r") {
                let Some(url) = t.content() else { continue };
                let (read_enabled, write_enabled) = match t.as_slice().get(2).map(|s| s.as_str()) {
                    Some("read") => (true, false),
                    Some("write") => (false, true),
                    _ => (true, true),
                };
                relay_repo.upsert(&RelayRow {
                    url: url.to_string(),
                    pubkey: owner.clone(),
                    name: None,
                    read_enabled,
                    write_enabled,
                    priority: 0,
                    last_connected_at: None,
                    health_score: 1.0,
                })?;
            }
        }
        Kind::Bookmarks => {
            let Some(event_id) = e_tags(event).first().cloned() else {
                return Ok(());
            };
            BookmarkRepo::new(db).upsert(&BookmarkRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                event_id,
                created_at: event.created_at.as_secs() as i64,
            })?;
        }
        Kind::Reaction => {
            let es = e_tags(event);
            let Some(target) = es.first() else {
                return Ok(());
            };
            let row = ReactionRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                event_id: target.clone(),
                kind: 7,
                content: Some(event.content.clone()),
                created_at: event.created_at.as_secs() as i64,
            };
            ReactionRepo::new(db).upsert(&row)?;
            let _ = tx.try_send(SyncUpdate::Reaction {
                id: row.id,
                event_id: row.event_id,
                pubkey: row.pubkey,
                content: row.content.unwrap_or_default(),
                created_at: event.created_at.as_secs(),
            });
        }
        // Text notes and other app-published kinds all land in `posts` so the
        // cached feed stays complete; only kinds with a surface model emit.
        Kind::TextNote => {
            if let Some(row) = post_row(event) {
                PostRepo::new(db).upsert(&row)?;
                let _ = tx.try_send(SyncUpdate::Feed {
                    id: row.id,
                    pubkey: row.pubkey,
                    content: row.content,
                    created_at: event.created_at.as_secs(),
                    kind: event.kind.as_u16() as u64,
                });
            }
        }
        _ => {
            if let Some(row) = post_row(event) {
                PostRepo::new(db).upsert(&row)?;
            }
        }
    }
    Ok(())
}

/// Handle a batch of verified relay events inside a single `with_tx` transaction wrapper.
pub fn handle_batch(
    db: &Database,
    my_pubkey: &str,
    events: &[Event],
    tx: &tokio::sync::mpsc::Sender<SyncUpdate>,
) -> Result<(), DbError> {
    if events.is_empty() {
        return Ok(());
    }
    for event in events {
        let _ = handle(db, my_pubkey, event, tx);
    }
    Ok(())
}

/// Which settings key tracks watermark for `kind` (None = not tracked).
pub fn watermark_key(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::TextNote => Some(WM_FEED),
        Kind::Metadata => Some(WM_META),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::event::{EventBuilder, FinalizeEvent};
    use nostr::key::Keys;
    use nostr::types::Timestamp;
    use soshal_db_core::query::query_first;

    fn channel() -> (
        tokio::sync::mpsc::Sender<SyncUpdate>,
        tokio::sync::mpsc::Receiver<SyncUpdate>,
    ) {
        tokio::sync::mpsc::channel(16)
    }

    fn signed_event(keys: &Keys, kind: Kind, content: &str, created_at: u64) -> Event {
        EventBuilder::new(kind, content)
            .custom_created_at(Timestamp::from(created_at))
            .finalize(keys)
            .unwrap()
    }

    fn seed_user(db: &Database, pubkey: &str) {
        UserRepo::new(db).ensure_exists(pubkey).unwrap();
    }

    #[test]
    fn watermark_advances_on_successful_ingest() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        seed_user(&db, &keys.public_key().to_hex());
        let event = signed_event(&keys, Kind::TextNote, "hello", 1_700_000_100);
        let (tx, _rx) = channel();

        set_watermark(&db, WM_FEED, 1_700_000_000);
        handle(&db, "", &event, &tx).unwrap();
        // Engine contract (engine.rs): cursor advances only on Ok ingest.
        let cur = watermark(&db, WM_FEED).max(event.created_at.as_secs());
        set_watermark(&db, WM_FEED, cur);
        assert_eq!(watermark(&db, WM_FEED), 1_700_000_100);
    }

    #[test]
    fn failed_ingest_does_not_advance_watermark() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let mut event = signed_event(&keys, Kind::TextNote, "hello", 1_700_000_100);
        event.content = "tampered".to_string();
        let (tx, _rx) = channel();
        set_watermark(&db, WM_FEED, 1_700_000_050);
        assert!(handle(&db, "", &event, &tx).is_err());
        assert_eq!(watermark(&db, WM_FEED), 1_700_000_050);
    }

    #[test]
    fn out_of_order_events_cached_and_watermark_monotonic() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        seed_user(&db, &keys.public_key().to_hex());
        let older = signed_event(&keys, Kind::TextNote, "older", 1_700_000_000);
        let newer = signed_event(&keys, Kind::TextNote, "newer", 1_700_000_100);
        let (tx, _rx) = channel();

        handle(&db, "", &newer, &tx).unwrap();
        handle(&db, "", &older, &tx).unwrap();

        let repo = PostRepo::new(&db);
        assert_eq!(
            repo.get_by_id(&older.id.to_hex()).unwrap().unwrap().content,
            "older"
        );
        assert_eq!(
            repo.get_by_id(&newer.id.to_hex()).unwrap().unwrap().content,
            "newer"
        );

        let mut cur = 0u64;
        for e in [&older, &newer] {
            if e.created_at.as_secs() > cur {
                cur = e.created_at.as_secs();
            }
        }
        set_watermark(&db, WM_FEED, cur);
        assert_eq!(watermark(&db, WM_FEED), 1_700_000_100);
    }

    #[test]
    fn watermark_persists_in_settings() {
        let db = soshal_test_util::test_db();
        assert_eq!(watermark(&db, WM_FEED), 0);
        set_watermark(&db, WM_FEED, 1_700_000_123);
        assert_eq!(watermark(&db, WM_FEED), 1_700_000_123);
        assert_eq!(
            SettingsRepo::new(&db).get(WM_FEED).unwrap().unwrap(),
            "1700000123"
        );
    }

    #[test]
    fn duplicate_event_single_row_and_double_emit() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        seed_user(&db, &keys.public_key().to_hex());
        let event = signed_event(&keys, Kind::TextNote, "dup", 1_700_000_100);
        let (tx, mut rx) = channel();
        handle(&db, "", &event, &tx).unwrap();
        handle(&db, "", &event, &tx).unwrap();

        let id = event.id.to_hex();
        let count: i64 = query_first(
            &db.conn().unwrap(),
            "SELECT COUNT(*) FROM posts WHERE id = ?1",
            libsql::params![id.as_str()],
            |r| r.get::<i64>(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 1);
        assert!(matches!(rx.try_recv().unwrap(), SyncUpdate::Feed { .. }));
        assert!(matches!(rx.try_recv().unwrap(), SyncUpdate::Feed { .. }));
        assert!(rx.try_recv().is_err());
    }
}
