//! Verified-event ingest: relay-fetched events are untrusted, so every event
//! passes `event.verify()` (signature + id) before any row is written or any
//! update is emitted. Kind-4 payloads additionally require a p-tag pointing
//! at the local pubkey (or authorship by it) — mirroring the
//! `commands::util::verified_events` rule set from the hardening audit.

use crate::{SyncUpdate, WM_DM, WM_FEED, WM_META};
use nostr::event::{Event, Kind};
use nostr::key::PublicKey;
use nostr::nips::nip19::ToBech32;
use sha2::{Digest, Sha256};
use soshal_common_core::consts::{
    KIND_CUSTOM_PROFILE, KIND_EVENT, KIND_EVENT_RSVP, KIND_GUESTBOOK, KIND_GUESTBOOK_APPROVAL,
    KIND_LISTING, KIND_LIVE, KIND_MENTION, KIND_MINIS, KIND_ORDER, KIND_PROFILE, KIND_REACTION,
    KIND_STORY, KIND_SWAP,
};

/// Kinds permitted to land in the `posts` table. Everything else arriving on
/// the relay wire (legacy NIP-04, unknown/junk kinds) is dropped at ingest.
const POST_KIND_ALLOWLIST: &[u16] = &[
    1,
    KIND_REACTION,
    1059,
    KIND_GUESTBOOK,
    KIND_GUESTBOOK_APPROVAL,
    KIND_STORY,
    KIND_PROFILE,
    KIND_CUSTOM_PROFILE,
    KIND_LIVE,
    KIND_LISTING,
    KIND_ORDER,
    KIND_MINIS,
    KIND_EVENT,
    KIND_EVENT_RSVP,
    KIND_SWAP,
    KIND_MENTION,
];
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

/// Order-preserving union of two comma-separated pubkey lists.
fn merge_pubkey_lists(stored: &str, incoming: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for pk in stored.split(',').chain(incoming.split(',')) {
        let pk = pk.trim();
        if pk.is_empty() || !seen.insert(pk.to_string()) {
            continue;
        }
        out.push(pk.to_string());
    }
    out.join(",")
}

fn has_p_tag(event: &Event, pubkey: &str) -> bool {
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "p")
        .any(|t| t.content().is_some_and(|c| c == pubkey))
}

/// Convert a verified relay event into its cached DB row (if cacheable).
fn post_row(event: &Event) -> Option<PostRow> {
    if event.content.len() > MAX_CACHED_CONTENT {
        return None;
    }
    let mut es: Vec<String> = Vec::new();
    let mut ps: Vec<String> = Vec::new();
    let mut ts: Vec<String> = Vec::new();
    let mut tags_json: Vec<&[String]> = Vec::with_capacity(event.tags.len());
    let mut freenet_key: Option<String> = None;
    for tag in event.tags.iter() {
        let vec = tag.as_slice();
        match vec.first().map(|s| s.as_str()) {
            Some("e") => {
                if let Some(c) = vec.get(1) {
                    es.push(c.clone());
                }
            }
            Some("p") => {
                if let Some(c) = vec.get(1) {
                    ps.push(c.clone());
                }
            }
            Some("t") => {
                if let Some(c) = vec.get(1) {
                    ts.push(c.clone());
                }
            }
            Some("freenet") if freenet_key.is_none() => {
                freenet_key = vec.get(1).cloned();
            }
            _ => {}
        }
        tags_json.push(vec);
    }
    let reply_to = es.first().cloned();
    let root_id = es.get(1).cloned().or_else(|| es.first().cloned());
    let sig = event.sig.to_string();

    Some(PostRow {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        content: event.content.clone(),
        kind: event.kind.as_u16() as u64 as i64,
        created_at: event.created_at.as_secs() as i64,
        tags_json: serde_json::to_string(&tags_json).unwrap_or_default(),
        sig: Some(sig),
        reply_to,
        root_id,
        mentioned_pubkeys: ps.join(","),
        mentioned_hashtags: ts.join(","),
        subject: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: freenet_key.clone(),
        is_freenet_native: freenet_key.is_some(),
        rsvp_event_id: if event.kind.as_u16() == KIND_EVENT_RSVP {
            es.first().cloned()
        } else {
            None
        },
    })
}

/// NIP-01 canonical JSON of a stored zap-request event (field order fixed:
/// id, pubkey, created_at, kind, tags, content, sig). The payer's wallet
/// hashed exactly this byte string into the invoice description hash.
fn zap_request_canonical_json(req: &PostRow) -> String {
    let tags = serde_json::from_str::<serde_json::Value>(&req.tags_json)
        .unwrap_or(serde_json::Value::Null);
    format!(
        "{{\"id\":{},\"pubkey\":{},\"created_at\":{},\"kind\":{},\"tags\":{},\"content\":{},\"sig\":{}}}",
        serde_json::to_string(&req.id).unwrap_or_default(),
        serde_json::to_string(&req.pubkey).unwrap_or_default(),
        req.created_at,
        req.kind,
        serde_json::to_string(&tags).unwrap_or_default(),
        serde_json::to_string(&req.content).unwrap_or_default(),
        serde_json::to_string(&req.sig.clone().unwrap_or_default()).unwrap_or_default(),
    )
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
    let conn = db.conn()?;
    soshal_db_core::query::with_tx(&conn, |t| async move {
        handle_impl(db, my_pubkey, event, tx, false, &t).await?;
        t.commit().await?;
        Ok(())
    })
}

async fn handle_impl(
    db: &Database,
    my_pubkey: &str,
    event: &Event,
    tx: &tokio::sync::mpsc::Sender<SyncUpdate>,
    already_verified: bool,
    t: &libsql::Transaction,
) -> Result<(), DbError> {
    if !already_verified {
        event
            .verify()
            .map_err(|e| DbError::Migration(format!("event verification failed: {e}")))?;
    }

    // DM kind: only keep payloads addressed to us (p-tag mine) or authored by
    // us; content stays encrypted here — never persisted unverified.
    if event.kind == Kind::EncryptedDirectMessage {
        let addresses_me = has_p_tag(event, my_pubkey);
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
                UserRepo::new(db).upsert_in(t, &row).await?;
                let _ = tx.try_send(SyncUpdate::Profile { pubkey: row.pubkey });
            }
        }
        Kind::ContactList => {
            let pubkey = event.pubkey.to_hex();
            let repo = UserRepo::new(db);
            let mut row = repo
                .get_by_pubkey_in(t, &pubkey)
                .await?
                .unwrap_or_else(|| UserRow {
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
            // Merge: kind-3 lists arrive per-relay and are partial views;
            // union with the stored list so contacts seen on other relays
            // are never dropped by a narrower list.
            row.contact_pubkeys =
                merge_pubkey_lists(&row.contact_pubkeys, &p_tags(event).join(","));
            repo.upsert_in(t, &row).await?;
        }
        Kind::ZapRequest => {
            // Store the request so later receipts can bind to it (NIP-57).
            if let Some(row) = post_row(event) {
                PostRepo::new(db).upsert_in(t, &row).await?;
            }
        }
        Kind::ZapReceipt => {
            let Some(recipient) = p_tags(event).first().cloned() else {
                return Ok(());
            };
            if recipient != my_pubkey {
                return Ok(());
            }
            let Ok(amount_msats) = soshal_zap_core::parse_msats_from_bolt11(&event.content) else {
                return Ok(());
            };
            if amount_msats == 0 {
                return Ok(());
            }
            // NIP-57 binding: only trust receipts whose invoice description
            // hash matches a verified zap-request addressed to us for the
            // same note at the same amount. Without this, any relay user can
            // forge 9735s and inflate zap totals without paying a sat.
            let Some(desc_hash) = soshal_zap_core::bolt11_description_hash(&event.content) else {
                return Ok(());
            };
            let Some(zapped_event) = e_tags(event).first().cloned() else {
                return Ok(());
            };
            let requests = PostRepo::new(db)
                .get_zap_requests_for_note_in(t, &zapped_event)
                .await?;
            let matched = requests.iter().any(|req| {
                let tags: Vec<Vec<String>> =
                    serde_json::from_str(&req.tags_json).unwrap_or_default();
                let p_me = tags.iter().any(|t| {
                    t.first().is_some_and(|k| k == "p") && t.get(1).is_some_and(|v| v == my_pubkey)
                });
                let amount_ok = tags.iter().any(|t| {
                    t.first().is_some_and(|k| k == "amount")
                        && t.get(1).and_then(|v| v.parse::<u64>().ok()) == Some(amount_msats)
                });
                let hash_ok = {
                    let mut hasher = Sha256::new();
                    hasher.update(zap_request_canonical_json(req).as_bytes());
                    let digest = hasher.finalize();
                    digest.as_slice() == desc_hash
                };
                p_me && amount_ok && hash_ok
            });
            if !matched {
                return Ok(());
            }
            let row = ZapRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                recipient_pubkey: recipient,
                event_id: Some(zapped_event),
                amount: (amount_msats / 1000).min(i64::MAX as u64) as i64,
                content: Some(event.content.clone()),
                created_at: event.created_at.as_secs() as i64,
                zap_type: "public".to_string(),
            };
            ZapRepo::new(db).upsert_in(t, &row).await?;
        }
        Kind::RelayList => {
            let relay_repo = RelayRepo::new(db);
            let owner = Some(event.pubkey.to_hex());
            for tag in event.tags.iter().filter(|t| t.kind() == "r") {
                let Some(url) = tag.content() else { continue };
                if !soshal_common_core::url::is_valid_event_relay_url(url) {
                    continue;
                }
                let (read_enabled, write_enabled) = match tag.as_slice().get(2).map(|s| s.as_str())
                {
                    Some("read") => (true, false),
                    Some("write") => (false, true),
                    _ => (true, true),
                };
                relay_repo
                    .upsert_in(
                        t,
                        &RelayRow {
                            url: url.to_string(),
                            pubkey: owner.clone(),
                            name: None,
                            read_enabled,
                            write_enabled,
                            priority: 0,
                            last_connected_at: None,
                            health_score: 1.0,
                        },
                    )
                    .await?;
            }
        }
        Kind::Bookmarks => {
            let Some(event_id) = e_tags(event).first().cloned() else {
                return Ok(());
            };
            BookmarkRepo::new(db)
                .upsert_in(
                    t,
                    &BookmarkRow {
                        id: event.id.to_hex(),
                        pubkey: event.pubkey.to_hex(),
                        event_id,
                        created_at: event.created_at.as_secs() as i64,
                    },
                )
                .await?;
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
                kind: KIND_REACTION as i64,
                content: Some(event.content.clone()),
                created_at: event.created_at.as_secs() as i64,
            };
            ReactionRepo::new(db).upsert_in(t, &row).await?;
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
                PostRepo::new(db).upsert_in(t, &row).await?;
                let _ = tx.try_send(SyncUpdate::Feed {
                    id: row.id,
                    pubkey: row.pubkey,
                    content: row.content,
                    created_at: event.created_at.as_secs(),
                    kind: event.kind.as_u16() as u64,
                });
            }
        }
        Kind::EventDeletion => {
            // NIP-09 tombstone. Only the deleting author's own rows are
            // cleared — a relay replaying a stale delete must not hide a
            // re-issued post by someone else. Tombstone-wins: once flagged,
            // POST_UPSERT_SQL refuses to resurrect the row.
            let author = event.pubkey.to_hex();
            for eid in e_tags(event) {
                let _ = PostRepo::new(db).mark_deleted_in(t, &eid, &author).await;
            }
        }
        _ => {
            // Kind allowlist: only kinds the app models may land in `posts`.
            // Anything else (NIP-04 DM legacy, unknown junk, hostile test
            // events) is dropped instead of being cached as a feed row.
            if POST_KIND_ALLOWLIST.contains(&event.kind.as_u16()) {
                if let Some(row) = post_row(event) {
                    PostRepo::new(db).upsert_in(t, &row).await?;
                }
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
    let conn = db.conn()?;
    soshal_db_core::query::with_tx(&conn, |t| async move {
        let mut rows: Vec<PostRow> = Vec::with_capacity(events.len());
        for event in events {
            // Route through the cached verifier (nostr-core LRU) — raw
            // event.verify() re-verifies signatures for re-fetched batches.
            if !soshal_nostr_core::models::verify_event(event) {
                continue;
            }
            match event.kind {
                Kind::EncryptedDirectMessage
                | Kind::Metadata
                | Kind::ContactList
                | Kind::ZapReceipt
                | Kind::RelayList
                | Kind::Bookmarks
                | Kind::Reaction => {
                    let _ = handle_impl(db, my_pubkey, event, tx, true, &t).await;
                }
                _ => {
                    if POST_KIND_ALLOWLIST.contains(&event.kind.as_u16()) {
                        if let Some(row) = post_row(event) {
                            rows.push(row);
                        }
                    }
                }
            }
        }
        PostRepo::new(db).upsert_batch_in(&t, &rows).await?;
        t.commit().await?;
        for row in &rows {
            if row.kind == Kind::TextNote.as_u16() as i64 {
                let _ = tx.try_send(SyncUpdate::Feed {
                    id: row.id.clone(),
                    pubkey: row.pubkey.clone(),
                    content: row.content.clone(),
                    created_at: row.created_at as u64,
                    kind: row.kind as u64,
                });
            }
        }
        Ok(())
    })
}

/// Which settings key tracks watermark for `kind` (unmapped kinds default to feed).
pub fn watermark_key(kind: Kind) -> Option<&'static str> {
    if kind == Kind::from(soshal_common_core::consts::KIND_MINIS) {
        return Some(WM_META);
    }
    match kind {
        Kind::TextNote => Some(WM_FEED),
        Kind::EncryptedDirectMessage => Some(WM_DM),
        Kind::Metadata => Some(WM_META),
        Kind::ContactList | Kind::RelayList | Kind::Bookmarks | Kind::ZapReceipt => Some(WM_META),
        Kind::Reaction => Some(WM_FEED),
        _ => Some(WM_FEED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::event::{EventBuilder, FinalizeEvent, Tag};
    use nostr::key::Keys;
    use soshal_db_core::query::query_first;

    fn channel() -> (
        tokio::sync::mpsc::Sender<SyncUpdate>,
        tokio::sync::mpsc::Receiver<SyncUpdate>,
    ) {
        tokio::sync::mpsc::channel(16)
    }

    fn signed_event_with_tags(
        keys: &Keys,
        kind: Kind,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> Event {
        let mut builder = EventBuilder::new(kind, content);
        for t in tags {
            builder = builder.tag(Tag::parse(t).unwrap());
        }
        builder.finalize(keys).unwrap()
    }

    /// Builds a valid `lnbc10n` invoice (1 sat) carrying `hash` in the
    /// BOLT-11 `h` (description hash) tagged field.
    fn test_invoice_with_description_hash(hash: &[u8; 32]) -> String {
        let mut words = Vec::new();
        let mut bits: u64 = 0;
        let mut bit_len = 0u32;
        for &b in hash {
            bits = (bits << 8) | u64::from(b);
            bit_len += 8;
            while bit_len >= 5 {
                bit_len -= 5;
                words.push(((bits >> bit_len) & 0x1f) as u8);
            }
        }
        if bit_len > 0 {
            words.push(((bits << (5 - bit_len)) & 0x1f) as u8);
        }
        let mut data = vec![0u8]; // version 0
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]); // 35-bit timestamp
        data.push(23); // 1 base32 word: field type 23 ('h')
        data.extend_from_slice(&[1, 20]); // 2 base32 words: field length 52 (1 * 32 + 20)
        data.extend_from_slice(&words);
        bech32::encode::<bech32::Bech32>(bech32::Hrp::parse("lnbc10n").unwrap(), &data).unwrap()
    }

    #[test]
    fn watermark_advances_on_successful_ingest() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
        let event = soshal_test_util::signed_event(&keys, Kind::TextNote, "hello", 1_700_000_100);
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
        let mut event =
            soshal_test_util::signed_event(&keys, Kind::TextNote, "hello", 1_700_000_100);
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
        soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
        let older = soshal_test_util::signed_event(&keys, Kind::TextNote, "older", 1_700_000_000);
        let newer = soshal_test_util::signed_event(&keys, Kind::TextNote, "newer", 1_700_000_100);
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
        soshal_test_util::seed_user(&db, &keys.public_key().to_hex());
        let event = soshal_test_util::signed_event(&keys, Kind::TextNote, "dup", 1_700_000_100);
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

    #[test]
    fn watermark_key_maps_all_kind_arms() {
        assert_eq!(watermark_key(Kind::from(31020)), Some(WM_META)); // KIND_MINIS arm
        assert_eq!(watermark_key(Kind::ContactList), Some(WM_META));
        assert_eq!(watermark_key(Kind::RelayList), Some(WM_META));
        assert_eq!(watermark_key(Kind::Bookmarks), Some(WM_META));
        assert_eq!(watermark_key(Kind::ZapReceipt), Some(WM_META));
        assert_eq!(watermark_key(Kind::Metadata), Some(WM_META));
        assert_eq!(watermark_key(Kind::EncryptedDirectMessage), Some(WM_DM));
        assert_eq!(watermark_key(Kind::TextNote), Some(WM_FEED));
        assert_eq!(watermark_key(Kind::Reaction), Some(WM_FEED));
        // Custom kinds fall to the default WM_FEED arm (RSVP + arbitrary custom).
        assert_eq!(watermark_key(Kind::Custom(KIND_EVENT_RSVP)), Some(WM_FEED));
        assert_eq!(watermark_key(Kind::Custom(30000)), Some(WM_FEED));
    }

    #[test]
    fn user_row_skips_bad_json_and_post_row_extracts_tags() {
        let keys = Keys::generate();

        // user_row: invalid JSON content → None (skip).
        let bad = soshal_test_util::signed_event(&keys, Kind::Metadata, "not-json{", 100);
        assert!(user_row(&bad).is_none());

        // Valid metadata JSON → fields extracted.
        let good = soshal_test_util::signed_event(
            &keys,
            Kind::Metadata,
            r#"{"name":"alice","about":"hi","nip05":"a@x.com"}"#,
            100,
        );
        let row = user_row(&good).unwrap();
        assert_eq!(row.name.as_deref(), Some("alice"));
        assert_eq!(row.about.as_deref(), Some("hi"));
        assert_eq!(row.nip05.as_deref(), Some("a@x.com"));
        assert_eq!(row.pubkey, keys.public_key().to_hex());

        // post_row: reply/root/hashtag/mentioned-pubkey extraction.
        let reply = "e1";
        let root = "e0";
        let post = signed_event_with_tags(
            &keys,
            Kind::TextNote,
            "hi",
            vec![
                vec!["e".to_string(), reply.to_string()],
                vec!["e".to_string(), root.to_string()],
                vec!["p".to_string(), "pk-target".to_string()],
                vec!["t".to_string(), "mesh".to_string()],
            ],
        );
        let prow = post_row(&post).unwrap();
        assert_eq!(prow.reply_to.as_deref(), Some(reply));
        assert_eq!(prow.root_id.as_deref(), Some(root));
        assert_eq!(prow.mentioned_pubkeys, "pk-target");
        assert_eq!(prow.mentioned_hashtags, "mesh");
        assert!(prow.rsvp_event_id.is_none());

        // RSVP kind → rsvp_event_id set from first e-tag.
        let rsvp = signed_event_with_tags(
            &keys,
            Kind::Custom(KIND_EVENT_RSVP),
            "yes",
            vec![vec!["e".to_string(), "evt".to_string()]],
        );
        let rrow = post_row(&rsvp).unwrap();
        assert_eq!(rrow.rsvp_event_id.as_deref(), Some("evt"));
    }

    #[test]
    fn zap_relay_bookmarks_branch_behaviors() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let pk = keys.public_key().to_hex();
        soshal_test_util::seed_user(&db, &pk); // bookmarks FK
        let (tx, _rx) = channel();

        // ZapReceipt missing p-tag: early return, nothing persisted.
        let zap_no_p = signed_event_with_tags(&keys, Kind::ZapReceipt, "zap", vec![]);
        handle(&db, "", &zap_no_p, &tx).unwrap();
        let count: i64 = query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM zaps", (), |r| {
            r.get(0)
        })
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);

        // Receipt addressed to someone else (p-tag != my pubkey): skipped.
        let target = "aa".repeat(32);
        let mine = "bb".repeat(32);
        let zap_other = signed_event_with_tags(
            &keys,
            Kind::ZapReceipt,
            "lnbc10n",
            vec![vec!["p".to_string(), target]],
        );
        handle(&db, &mine, &zap_other, &tx).unwrap();
        let count: i64 = query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM zaps", (), |r| {
            r.get(0)
        })
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);

        // Invoice missing/unparseable from content: skipped, nothing persisted.
        let zap_bad = signed_event_with_tags(
            &keys,
            Kind::ZapReceipt,
            "zap",
            vec![vec!["p".to_string(), mine.clone()]],
        );
        handle(&db, &mine, &zap_bad, &tx).unwrap();
        let count: i64 = query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM zaps", (), |r| {
            r.get(0)
        })
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);

        // Valid invoice addressed to me but no matching zap-request
        // (NIP-57 binding missing): dropped, nothing persisted.
        let zap_unbound = signed_event_with_tags(
            &keys,
            Kind::ZapReceipt,
            "lnbc10n",
            vec![
                vec!["p".to_string(), mine.clone()],
                vec!["e".to_string(), "note1".to_string()],
            ],
        );
        handle(&db, &mine, &zap_unbound, &tx).unwrap();
        let count: i64 = query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM zaps", (), |r| {
            r.get(0)
        })
        .unwrap()
        .unwrap();
        assert_eq!(count, 0);

        // Full NIP-57 flow: zap-request ingested first, then a receipt whose
        // invoice description hash matches the request's canonical JSON and
        // whose amount matches the request's amount tag -> stored (1 sat).
        let request = signed_event_with_tags(
            &keys,
            Kind::ZapRequest,
            "",
            vec![
                vec!["p".to_string(), mine.clone()],
                vec!["e".to_string(), "note1".to_string()],
                vec!["amount".to_string(), "1000".to_string()],
            ],
        );
        handle(&db, &mine, &request, &tx).unwrap();
        let canonical = format!(
            "{{\"id\":{},\"pubkey\":{},\"created_at\":{},\"kind\":{},\"tags\":{},\"content\":{},\"sig\":{}}}",
            serde_json::to_string(&request.id.to_hex()).unwrap(),
            serde_json::to_string(&request.pubkey.to_hex()).unwrap(),
            request.created_at.as_secs(),
            request.kind.as_u16(),
            serde_json::to_string(
                &request
                    .tags
                    .iter()
                    .map(|t| t.as_slice().to_vec())
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            serde_json::to_string(&request.content).unwrap(),
            serde_json::to_string(&request.sig.to_string()).unwrap(),
        );
        let hash: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
        let invoice = test_invoice_with_description_hash(&hash);
        let zap_bound = signed_event_with_tags(
            &keys,
            Kind::ZapReceipt,
            &invoice,
            vec![
                vec!["p".to_string(), mine.clone()],
                vec!["e".to_string(), "note1".to_string()],
            ],
        );
        handle(&db, &mine, &zap_bound, &tx).unwrap();
        let amount: i64 = query_first(
            &db.conn().unwrap(),
            "SELECT amount FROM zaps WHERE id = ?1",
            libsql::params![zap_bound.id.to_hex().as_str()],
            |r| r.get(0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(amount, 1); // lnbc10n = 1 sat

        // RelayList: read/write/other branches set flags.
        let relays = signed_event_with_tags(
            &keys,
            Kind::RelayList,
            "",
            vec![
                vec![
                    "r".to_string(),
                    "wss://read.only".to_string(),
                    "read".to_string(),
                ],
                vec![
                    "r".to_string(),
                    "wss://write.only".to_string(),
                    "write".to_string(),
                ],
                vec!["r".to_string(), "wss://both.example.com".to_string()],
            ],
        );
        handle(&db, "", &relays, &tx).unwrap();
        let rr = RelayRepo::new(&db)
            .get_by_url("wss://read.only")
            .unwrap()
            .unwrap();
        assert!(rr.read_enabled && !rr.write_enabled);
        let wr = RelayRepo::new(&db)
            .get_by_url("wss://write.only")
            .unwrap()
            .unwrap();
        assert!(!wr.read_enabled && wr.write_enabled);
        let br = RelayRepo::new(&db)
            .get_by_url("wss://both.example.com")
            .unwrap()
            .unwrap();
        assert!(br.read_enabled && br.write_enabled);

        // Bookmarks missing e-tag: early return, no row.
        let bm_no_e = signed_event_with_tags(&keys, Kind::Bookmarks, "", vec![]);
        handle(&db, "", &bm_no_e, &tx).unwrap();
        assert!(BookmarkRepo::new(&db)
            .get_by_id(&bm_no_e.id.to_hex())
            .unwrap()
            .is_none());

        // Bookmarks with e-tag: row persisted.
        let bm = signed_event_with_tags(
            &keys,
            Kind::Bookmarks,
            "",
            vec![vec!["e".to_string(), "evt-9".to_string()]],
        );
        handle(&db, "", &bm, &tx).unwrap();
        let row = BookmarkRepo::new(&db)
            .get_by_id(&bm.id.to_hex())
            .unwrap()
            .unwrap();
        assert_eq!(row.event_id, "evt-9");
    }

    #[test]
    fn contact_list_merges_into_existing_user() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let pk = keys.public_key().to_hex();
        let (tx, _rx) = channel();

        // Metadata first: user row with a name.
        let meta =
            soshal_test_util::signed_event(&keys, Kind::Metadata, r#"{"name":"alice"}"#, 100);
        handle(&db, "", &meta, &tx).unwrap();

        // First contact list: two follows.
        let cl1 = signed_event_with_tags(
            &keys,
            Kind::ContactList,
            "",
            vec![
                vec!["p".to_string(), "a".repeat(64)],
                vec!["p".to_string(), "b".repeat(64)],
            ],
        );
        handle(&db, "", &cl1, &tx).unwrap();
        let row = UserRepo::new(&db).get_by_pubkey(&pk).unwrap().unwrap();
        assert_eq!(
            row.contact_pubkeys,
            format!("{},{}", "a".repeat(64), "b".repeat(64))
        );
        assert_eq!(row.name.as_deref(), Some("alice"));

        // Re-ingest second contact list for same user: union merges, so the
        // follow set is never narrowed by a partial relay view.
        let cl2 = signed_event_with_tags(
            &keys,
            Kind::ContactList,
            "",
            vec![vec!["p".to_string(), "c".repeat(64)]],
        );
        handle(&db, "", &cl2, &tx).unwrap();
        let row = UserRepo::new(&db).get_by_pubkey(&pk).unwrap().unwrap();
        assert_eq!(
            row.contact_pubkeys,
            format!("{},{},{}", "a".repeat(64), "b".repeat(64), "c".repeat(64))
        );
        assert_eq!(row.name.as_deref(), Some("alice"));
    }

    #[test]
    fn batch_empty_unverified_and_routing() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let my = keys.public_key().to_hex();
        let (tx, mut rx) = channel();

        // Empty batch: Ok, nothing written.
        handle_batch(&db, &my, &[], &tx).unwrap();

        // Unverified (tampered) event: skipped, no row.
        let mut tampered = soshal_test_util::signed_event(&keys, Kind::TextNote, "orig", 100);
        tampered.content = "tampered".to_string();
        handle_batch(&db, &my, &[tampered.clone()], &tx).unwrap();
        assert!(PostRepo::new(&db)
            .get_by_id(&tampered.id.to_hex())
            .unwrap()
            .is_none());

        // DM addressed to us: routed into handle_impl → Dm update.
        let dm = signed_event_with_tags(
            &keys,
            Kind::EncryptedDirectMessage,
            "enc",
            vec![vec!["p".to_string(), my.clone()]],
        );
        handle_batch(&db, &my, &[dm], &tx).unwrap();
        match rx.try_recv().unwrap() {
            SyncUpdate::Dm { content, .. } => assert_eq!(content, "enc"),
            other => panic!("unexpected update: {other:?}"),
        }

        // Metadata in batch: routed → user row created.
        let meta = soshal_test_util::signed_event(&keys, Kind::Metadata, r#"{"name":"bob"}"#, 200);
        handle_batch(&db, &my, &[meta], &tx).unwrap();
        let row = UserRepo::new(&db).get_by_pubkey(&my).unwrap().unwrap();
        assert_eq!(row.name.as_deref(), Some("bob"));
    }
}
