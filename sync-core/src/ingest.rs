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
    KIND_STORY, KIND_SWAP, MAX_TAGS,
};

/// Clock-skew grace applied when clamping relay-supplied timestamps (secs).
const TS_GRACE_SECS: u64 = 300;

/// Clamp a relay-supplied `created_at` (secs) to `[0, now + grace]` as i64.
/// Relay events are untrusted: a far-future `created_at` would pin the
/// author's content at the top of every `ORDER BY created_at DESC` feed and
/// jam the sync watermark past all subsequent legit events. Storage never
/// exceeds wall clock + grace; negative/pre-epoch timestamps clamp to 0.
fn sanitize_ts(secs: u64) -> i64 {
    let now = soshal_common_core::format::now_secs().max(0) as u64;
    secs.min(now.saturating_add(TS_GRACE_SECS)) as i64
}

/// u64 variant for channels/watermarks that carry `created_at` as u64.
fn sanitize_ts_u64(secs: u64) -> u64 {
    let now = soshal_common_core::format::now_secs().max(0) as u64;
    secs.min(now.saturating_add(TS_GRACE_SECS))
}

/// Kinds permitted to land in the `posts` table. Everything else arriving on
/// the relay wire (legacy NIP-04, unknown/junk kinds) is dropped at ingest.
const POST_KIND_ALLOWLIST: &[u16] = &[
    1,
    6,
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
    9734,
];
use soshal_db_core::error::DbError;
use soshal_db_core::repos::bookmark::{BookmarkRepo, BookmarkRow};
use soshal_db_core::repos::hashtag::{HashtagRepo, HashtagRow};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::repos::post::{PostRepo, PostRow};
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::repos::relay::{RelayRepo, RelayRow};
use soshal_db_core::repos::repost::{RepostRepo, RepostRow};
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

fn first_e_tag(event: &Event) -> Option<&str> {
    event
        .tags
        .iter()
        .find(|t| t.kind() == "e")
        .and_then(|t| t.content())
}

fn p_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "p")
        .filter_map(|t| t.content().map(|c| c.to_string()))
        .collect()
}

fn r_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "r")
        .filter_map(|t| t.content().map(|c| c.to_string()))
        .collect()
}

/// Parse a stored/incoming pubkey list. The canonical storage format is a
/// JSON array (`["a","b"]`, written by identity/kind-1 flows and read via
/// `json_each`/`serde_json::from_str::<Vec<String>>`), but legacy rows and the
/// per-event p-tag join are comma-separated. Accept both.
fn split_pubkey_list(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Vec::new();
    }
    if trimmed.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<Vec<String>>(trimmed) {
            return v;
        }
    }
    trimmed
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect()
}

fn has_p_tag(event: &Event, pubkey: &str) -> bool {
    if pubkey.is_empty() {
        return false;
    }
    event
        .tags
        .iter()
        .filter(|t| t.kind() == "p")
        .any(|t| t.content().is_some_and(|c| c.eq_ignore_ascii_case(pubkey)))
}

/// Author pubkey of a cached post, by id. Used by the notification producer
/// to decide whether a reaction/reply/repost targets the local user's content.
async fn post_author_id(
    t: &libsql::Transaction,
    event_id: &str,
) -> Result<Option<String>, DbError> {
    let stmt = t
        .prepare("SELECT pubkey FROM posts WHERE LOWER(id) = LOWER(?1) AND is_deleted = 0")
        .await?;
    let mut rows = stmt.query(libsql::params![event_id]).await?;
    let Ok(Some(row)) = rows.next().await else {
        return Ok(None);
    };
    let pk: String = row.get(0)?;
    Ok(Some(pk))
}

/// Write a notification row for `event` addressed to the local user, mirroring
/// the aggregator's canonical id scheme (`notif_id`) and wording. Self-actions
/// never notify. Oversized payloads are skipped by the repo.
async fn notify_me(
    db: &Database,
    my_pubkey: &str,
    t: &libsql::Transaction,
    event: &Event,
    notif_type: &str,
    event_id: &str,
    content: &str,
) -> Result<(), DbError> {
    if event.pubkey.to_hex().eq_ignore_ascii_case(my_pubkey) {
        return Ok(());
    }
    let from_pubkey = event.pubkey.to_hex();
    let formatted =
        soshal_notification_core::aggregator::format_notification_content(notif_type, content, &[]);
    let row = NotificationRow {
        id: soshal_notification_core::events::notif_id(notif_type, event_id, &from_pubkey),
        pubkey: my_pubkey.to_string(),
        type_: notif_type.to_string(),
        event_id: Some(event_id.to_string()),
        from_pubkey: Some(from_pubkey),
        content: Some(formatted),
        created_at: sanitize_ts(event.created_at.as_secs()),
        is_read: false,
    };
    NotificationRepo::new(db).upsert_batch_in(t, &[row]).await
}

/// Convert a verified relay event into its cached DB row (if cacheable).
fn post_row(event: &Event) -> Option<PostRow> {
    if event.content.len() > MAX_CACHED_CONTENT {
        return None;
    }
    let mut marked_root: Option<String> = None;
    let mut marked_reply: Option<String> = None;
    let mut first_e: Option<String> = None;
    let mut last_e: Option<String> = None;
    let mut ps: Vec<&str> = Vec::new();
    let mut ts: Vec<&str> = Vec::new();
    let mut tags_json: Vec<&[String]> = Vec::with_capacity(event.tags.len());
    let mut freenet_key: Option<String> = None;
    for tag in event.tags.iter().take(MAX_TAGS) {
        let vec = tag.as_slice();
        match vec.first().map(|s| s.as_str()) {
            Some("e") => {
                if let Some(c) = vec.get(1) {
                    let marker = vec.get(3).map(|s| s.as_str());
                    if marker == Some("root") {
                        marked_root = Some(c.clone());
                    } else if marker == Some("reply") {
                        marked_reply = Some(c.clone());
                    } else if marker != Some("mention") {
                        if first_e.is_none() {
                            first_e = Some(c.clone());
                        }
                        last_e = Some(c.clone());
                    }
                }
            }
            Some("p") => {
                if let Some(c) = vec.get(1) {
                    ps.push(c.as_str());
                }
            }
            Some("t") => {
                if let Some(c) = vec.get(1) {
                    ts.push(c.as_str());
                }
            }
            Some("freenet") if freenet_key.is_none() => {
                freenet_key = vec.get(1).cloned();
            }
            _ => {}
        }
        tags_json.push(vec);
    }
    let reply_to = if event.kind == Kind::TextNote || event.kind == Kind::ZapRequest {
        marked_reply.or(last_e).or_else(|| marked_root.clone())
    } else {
        None
    };
    let root_id = if event.kind == Kind::TextNote || event.kind == Kind::ZapRequest {
        marked_root.or(first_e.clone())
    } else {
        None
    };
    let sig = event.sig.to_string();

    Some(PostRow {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        content: event.content.clone(),
        kind: event.kind.as_u16() as u64 as i64,
        created_at: sanitize_ts(event.created_at.as_secs()),
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
            first_e
        } else {
            None
        },
    })
}

/// NIP-01 canonical JSON of a stored zap-request event (field order fixed:
/// id, pubkey, created_at, kind, tags, content, sig). The payer's wallet
/// hashed exactly this byte string into the invoice description hash.
fn zap_request_canonical_json(req: &PostRow) -> String {
    let raw_tags = req.tags_json.trim();
    let tags_slice = if raw_tags.is_empty() || !raw_tags.starts_with('[') {
        "[]"
    } else {
        raw_tags
    };
    format!(
        "{{\"id\":{},\"pubkey\":{},\"created_at\":{},\"kind\":{},\"tags\":{},\"content\":{},\"sig\":{}}}",
        serde_json::to_string(&req.id).unwrap_or_default(),
        serde_json::to_string(&req.pubkey).unwrap_or_default(),
        req.created_at,
        req.kind,
        tags_slice,
        serde_json::to_string(&req.content).unwrap_or_default(),
        serde_json::to_string(&req.sig.as_deref().unwrap_or_default()).unwrap_or_default(),
    )
}

/// Digest cache for zap-request canonical JSON. The canonical form is
/// deterministic per stored request row, so receipt binding (which walks
/// every stored request for the note) re-serialized + re-hashed each request
/// per receipt — O(requests × receipts) SHA-256 runs. Bounded; cleared on
/// overflow.
static ZAP_REQUEST_DIGEST_CACHE: std::sync::Mutex<
    Option<std::collections::HashMap<String, [u8; 32]>>,
> = std::sync::Mutex::new(None);
const ZAP_REQUEST_DIGEST_CACHE_CAP: usize = 2048;

fn zap_request_digest(req: &PostRow) -> [u8; 32] {
    let mut guard = ZAP_REQUEST_DIGEST_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(Default::default);
    if let Some(d) = cache.get(&req.id) {
        return *d;
    }
    if cache.len() >= ZAP_REQUEST_DIGEST_CACHE_CAP {
        cache.clear();
    }
    let digest: [u8; 32] = {
        let mut hasher = Sha256::new();
        hasher.update(zap_request_canonical_json(req).as_bytes());
        hasher.finalize().into()
    };
    cache.insert(req.id.clone(), digest);
    digest
}

/// Ensure a users row exists for an event author so child rows referencing
/// users(pubkey) can be inserted under the enforced foreign_keys pragma.
/// INSERT OR IGNORE keeps any richer profile row already ingested.
async fn ensure_author_user(
    db: &Database,
    t: &libsql::Transaction,
    event: &Event,
) -> Result<(), DbError> {
    let _ = db;
    let pubkey = event.pubkey.to_hex();
    t.execute(
        "INSERT OR IGNORE INTO users (pubkey, npub, created_at, updated_at, contact_pubkeys, relay_list, follower_count) \
         VALUES (?1, ?2, ?3, ?3, '', '[]', 0)",
        libsql::params![pubkey.as_str(), "".to_string(), sanitize_ts(event.created_at.as_secs())],
    )
    .await?;
    Ok(())
}

fn user_row(event: &Event) -> Option<UserRow> {
    let meta: serde_json::Value = serde_json::from_str(&event.content).ok()?;
    let pubkey = event.pubkey.to_hex();
    let npub = PublicKey::from_hex(&pubkey)
        .ok()
        .map(|p| p.to_bech32().unwrap_or_default())
        .unwrap_or_default();
    let created = sanitize_ts(event.created_at.as_secs());
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
            .and_then(|s| soshal_common_core::url::is_valid_media_url(s).then(|| s.to_string())),
        banner: meta
            .get("banner")
            .and_then(|v| v.as_str())
            .and_then(|s| soshal_common_core::url::is_valid_media_url(s).then(|| s.to_string())),
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
        follower_count: 0,
    })
}

/// Extract bolt11 invoice from a zap receipt: check "bolt11" tag first (per NIP-57),
/// falling back to event content.
fn bolt11_from_event(event: &Event) -> Option<&str> {
    event
        .tags
        .iter()
        .find(|t| t.as_slice().first().map(|s| s == "bolt11").unwrap_or(false))
        .and_then(|t| t.as_slice().get(1).map(|s| s.as_str()))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let c = event.content.trim();
            if c.starts_with("lnbc") || c.starts_with("LNBC") {
                Some(c)
            } else {
                None
            }
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
    if let Err(e) = SettingsRepo::new(db).set(key, &ts.to_string()) {
        eprintln!("settings persist: {e}");
    }
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

/// Text-note notifications for events touching the local user:
/// friend-request (tag), reply (targets our post), else mention (p-tag
/// references us). Precedence mirrors the aggregator. Shared by the
/// single-event path (`handle_impl`) and the batch path (`handle_batch`) so
/// batch-ingested text notes emit the same notifications as single ingest.
async fn maybe_notify_text_note(
    db: &Database,
    my_pubkey: &str,
    t: &libsql::Transaction,
    event: &Event,
    row: &PostRow,
) -> Result<(), DbError> {
    let author = event.pubkey.to_hex();
    if !author.eq_ignore_ascii_case(my_pubkey) {
        let friend_request = event.tags.iter().any(|t| {
            t.kind() == "t"
                && t.content()
                    .is_some_and(|c| c.eq_ignore_ascii_case("friend-request"))
        });
        let reply_target = row.reply_to.as_deref().or(row.root_id.as_deref());
        let reply_to_me = match reply_target {
            Some(rt) => post_author_id(t, rt)
                .await?
                .is_some_and(|a| a.eq_ignore_ascii_case(my_pubkey)),
            None => false,
        };
        if friend_request && has_p_tag(event, my_pubkey) {
            notify_me(
                db,
                my_pubkey,
                t,
                event,
                "friend_request",
                &row.id,
                &row.content,
            )
            .await?;
        } else if reply_to_me {
            notify_me(
                db,
                my_pubkey,
                t,
                event,
                "reply",
                reply_target.unwrap_or(&row.id),
                &row.content,
            )
            .await?;
        } else if has_p_tag(event, my_pubkey) {
            let target = first_e_tag(event)
                .map(|s| s.to_string())
                .unwrap_or_else(|| event.id.to_hex());
            notify_me(db, my_pubkey, t, event, "mention", &target, &row.content).await?;
        }
    }
    Ok(())
}

async fn handle_impl(
    db: &Database,
    my_pubkey: &str,
    event: &Event,
    tx: &tokio::sync::mpsc::Sender<SyncUpdate>,
    already_verified: bool,
    t: &libsql::Transaction,
) -> Result<(), DbError> {
    if !already_verified && !soshal_nostr_core::models::verify_event(event) {
        return Err(DbError::Migration("event verification failed".to_string()));
    }

    // DM kind: only keep payloads addressed to us (p-tag mine) or authored by
    // us; content stays encrypted here — never persisted unverified.
    if event.kind == Kind::EncryptedDirectMessage {
        let addresses_me = has_p_tag(event, my_pubkey);
        let authored_by_me = event.pubkey.to_hex().eq_ignore_ascii_case(my_pubkey);
        if addresses_me || authored_by_me {
            let recipient = event
                .tags
                .iter()
                .find(|t| t.as_slice().first().map(|s| s == "p").unwrap_or(false))
                .and_then(|t| t.as_slice().get(1).cloned())
                .unwrap_or_else(|| my_pubkey.to_string());
            if let Err(e) = tx.try_send(SyncUpdate::Dm {
                id: event.id.to_hex(),
                sender: event.pubkey.to_hex(),
                recipient,
                content: event.content.clone(),
                created_at: sanitize_ts_u64(event.created_at.as_secs()),
                tags_json: serde_json::to_string(&event.tags).unwrap_or_else(|_| "[]".to_string()),
            }) {
                // Fail closed instead of advancing the DM watermark past a
                // message the consumer never received: the bridge persists the
                // DM only when this update is actually delivered, so dropping
                // it here permanently loses the message. Err → event excluded
                // from ok_pos → refetched on the next pass.
                eprintln!("sync engine: dm notification dropped: channel full: {e}");
                return Err(DbError::Migration(
                    "DM notification channel full; not advancing watermark".to_string(),
                ));
            }
        }
        return Ok(());
    }

    match event.kind {
        Kind::Metadata => {
            if let Some(row) = user_row(event) {
                let repo = UserRepo::new(db);
                let pubkey = row.pubkey.clone();
                let mut applied = false;
                match repo.get_by_pubkey_in(t, &row.pubkey).await? {
                    None => {
                        repo.upsert_in(t, &row).await?;
                        applied = true;
                    }
                    Some(stored) => {
                        // Newer-wins: never regress a stored profile with an
                        // older metadata replay (relays can redeliver stale
                        // copies out of order). A kind-0 carries no follow
                        // graph, so preserve the contacts/relays that kind-3
                        // merges wrote — a profile redelivery must not wipe
                        // the merged list.
                        if row.updated_at > stored.updated_at {
                            let mut updated = row;
                            updated.contact_pubkeys = stored.contact_pubkeys;
                            updated.relay_list = stored.relay_list;
                            repo.upsert_in(t, &updated).await?;
                            applied = true;
                        }
                    }
                }
                if applied && tx.try_send(SyncUpdate::Profile { pubkey }).is_err() {
                    eprintln!("sync update channel full, dropping update");
                }
            }
        }
        Kind::ContactList => {
            let pubkey = event.pubkey.to_hex();
            let repo = UserRepo::new(db);
            let existing = repo.get_by_pubkey_in(t, &pubkey).await?;
            let stored_list = existing
                .as_ref()
                .map(|r| r.contact_pubkeys.clone())
                .unwrap_or_default();
            let mut row = existing.unwrap_or_else(|| UserRow {
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
                created_at: sanitize_ts(event.created_at.as_secs()),
                updated_at: sanitize_ts(event.created_at.as_secs()),
                metadata_json: None,
                contact_pubkeys: String::new(),
                relay_list: String::new(),
                follower_count: 0,
            });
            row.contact_pubkeys = serde_json::to_string(&p_tags(event)).unwrap_or_default();
            row.relay_list = serde_json::to_string(&r_tags(event)).unwrap_or_default();
            repo.upsert_in(t, &row).await?;
            // Materialized follower counts: bump +1 for each pubkey newly
            // added to this author's list.
            let before: std::collections::HashSet<String> =
                split_pubkey_list(&stored_list).into_iter().collect();
            let new_list = split_pubkey_list(&row.contact_pubkeys);
            for added in &new_list {
                if added != &pubkey && !before.contains(added) {
                    UserRepo::new(db)
                        .bump_follower_count_in(t, added, 1)
                        .await?;
                }
            }
            let new: std::collections::HashSet<String> = new_list.into_iter().collect();
            for removed in before.difference(&new) {
                if removed != &pubkey {
                    UserRepo::new(db)
                        .bump_follower_count_in(t, removed, -1)
                        .await?;
                }
            }
            // "follow" notifications: only when the local user is newly on
            // this author's list (new follows, not re-syncs of the same list).
            if !pubkey.eq_ignore_ascii_case(my_pubkey)
                && has_p_tag(event, my_pubkey)
                && !before.iter().any(|p| p.eq_ignore_ascii_case(my_pubkey))
            {
                notify_me(db, my_pubkey, t, event, "follow", &event.id.to_hex(), "").await?;
            }
        }
        Kind::ZapRequest => {
            // Store the request so later receipts can bind to it (NIP-57).
            if let Some(row) = post_row(event) {
                ensure_author_user(db, t, event).await?;
                PostRepo::new(db).upsert_in(t, &row).await?;
            }
        }
        Kind::ZapReceipt => {
            if !has_p_tag(event, my_pubkey) {
                return Ok(());
            }
            let recipient = my_pubkey.to_string();
            let Some(bolt11) = bolt11_from_event(event) else {
                return Ok(());
            };
            let Ok(amount_msats) = soshal_zap_core::parse_msats_from_bolt11(bolt11) else {
                return Ok(());
            };
            if amount_msats == 0 {
                return Ok(());
            }
            // Checksum-forged invoice strings (invalid bech32) are rejected:
            // a real Lightning invoice always carries a valid checksum, so a
            // receipt whose bolt11 fails decode is not a genuine zap.
            if !soshal_zap_core::bolt11_checksum_valid(bolt11) {
                return Ok(());
            }
            // NIP-57 binding: only trust receipts whose invoice description
            // hash matches a verified zap-request addressed to us for the
            // same note at the same amount. Without this, any relay user can
            // forge 9735s and inflate zap totals without paying a sat.
            let Some(desc_hash) = soshal_zap_core::bolt11_description_hash(bolt11) else {
                return Ok(());
            };
            let Some(zapped_event) = first_e_tag(event) else {
                return Ok(());
            };
            let zapped_event = zapped_event.to_string();
            let requests = PostRepo::new(db)
                .get_zap_requests_for_note_in(t, &zapped_event)
                .await?;
            let matched = requests.iter().any(|req| {
                let tags: Vec<Vec<String>> =
                    serde_json::from_str(&req.tags_json).unwrap_or_default();
                let p_me = tags.iter().any(|t| {
                    t.first().is_some_and(|k| k == "p")
                        && t.get(1).is_some_and(|v| v.eq_ignore_ascii_case(my_pubkey))
                });
                let amount_ok = tags.iter().any(|t| {
                    t.first().is_some_and(|k| k == "amount")
                        && t.get(1).and_then(|v| v.parse::<u64>().ok()) == Some(amount_msats)
                });
                let hash_ok = zap_request_digest(req).as_slice() == desc_hash;
                p_me && amount_ok && hash_ok
            });
            if !matched {
                return Ok(());
            }
            let row = ZapRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                recipient_pubkey: recipient,
                event_id: Some(zapped_event.clone()),
                amount: amount_msats.div_ceil(1000).min(i64::MAX as u64) as i64,
                amount_msat: amount_msats.min(i64::MAX as u64) as i64,
                content: Some(event.content.clone()),
                created_at: sanitize_ts(event.created_at.as_secs()),
                zap_type: "public".to_string(),
            };
            ZapRepo::new(db).upsert_in(t, &row).await?;
            // Notification: only trusted, NIP-57-bound receipts reach here.
            notify_me(db, my_pubkey, t, event, "zap", &zapped_event, "").await?;
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
            let e_ids = e_tags(event);
            if e_ids.is_empty() {
                return Ok(());
            }
            ensure_author_user(db, t, event).await?;
            let pubkey = event.pubkey.to_hex();
            let created_at = sanitize_ts(event.created_at.as_secs());
            let repo = BookmarkRepo::new(db);
            for (idx, event_id) in e_ids.into_iter().enumerate() {
                let id = if idx == 0 {
                    event.id.to_hex()
                } else {
                    format!("{}:{event_id}", event.id.to_hex())
                };
                repo.upsert_in(
                    t,
                    &BookmarkRow {
                        id,
                        pubkey: pubkey.clone(),
                        event_id,
                        created_at,
                    },
                )
                .await?;
            }
            // Tombstoned targets: bookmarks are useless once the post is
            // deleted; sweep them whenever the list refreshes.
            let _ = repo.delete_for_deleted_targets_in(t, &pubkey).await;
        }
        Kind::Reaction => {
            let Some(target) = first_e_tag(event) else {
                return Ok(());
            };
            ensure_author_user(db, t, event).await?;
            let row = ReactionRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                event_id: target.to_string(),
                kind: KIND_REACTION as i64,
                content: Some(event.content.clone()),
                created_at: sanitize_ts(event.created_at.as_secs()),
            };
            ReactionRepo::new(db).upsert_in(t, &row).await?;
            // Notification: only when the reaction targets the local user's
            // own post (reactions to strangers are not ours to surface).
            if let Some(author) = post_author_id(t, &row.event_id).await? {
                if author.eq_ignore_ascii_case(my_pubkey) {
                    notify_me(
                        db,
                        my_pubkey,
                        t,
                        event,
                        "reaction",
                        &row.event_id,
                        row.content.as_deref().unwrap_or(""),
                    )
                    .await?;
                }
            }
            if tx
                .try_send(SyncUpdate::Reaction {
                    id: row.id,
                    event_id: row.event_id,
                    pubkey: row.pubkey,
                    content: row.content.unwrap_or_default(),
                    created_at: sanitize_ts_u64(event.created_at.as_secs()),
                })
                .is_err()
            {
                eprintln!("sync update channel full, dropping update");
            }
        }
        Kind::Repost => {
            let Some(target) = first_e_tag(event) else {
                return Ok(());
            };
            let target = target.to_string();
            ensure_author_user(db, t, event).await?;
            let row = RepostRow {
                id: event.id.to_hex(),
                pubkey: event.pubkey.to_hex(),
                event_id: target.clone(),
                created_at: sanitize_ts(event.created_at.as_secs()),
            };
            RepostRepo::new(db).upsert_in(t, &row).await?;
            // Notification: only when the repost targets the local user's post.
            if let Some(author) = post_author_id(t, &target).await? {
                if author.eq_ignore_ascii_case(my_pubkey) {
                    notify_me(db, my_pubkey, t, event, "repost", &target, "").await?;
                }
            }
        }
        // Text notes and other app-published kinds all land in `posts` so the
        // cached feed stays complete; only kinds with a surface model emit.
        Kind::TextNote => {
            if let Some(row) = post_row(event) {
                ensure_author_user(db, t, event).await?;
                PostRepo::new(db).upsert_in(t, &row).await?;
                let author = event.pubkey.to_hex();
                let created_at = sanitize_ts(event.created_at.as_secs());
                let mut seen_tags = std::collections::HashSet::new();
                for tag in soshal_content_core::hashtag::extract(&event.content)
                    .into_iter()
                    .filter(|t| seen_tags.insert(t.to_ascii_lowercase()))
                    .take(MAX_TAGS)
                {
                    HashtagRepo::new(db)
                        .upsert_in(
                            t,
                            &HashtagRow {
                                tag,
                                pubkey: author.clone(),
                                last_used_at: created_at,
                                count: 1,
                            },
                        )
                        .await?;
                }
                // Notifications for text notes touching the local user are
                // delegated to the shared helper so batch-ingested text notes
                // emit the same friend-request / reply / mention alerts.
                maybe_notify_text_note(db, my_pubkey, t, event, &row).await?;
                if tx
                    .try_send(SyncUpdate::Feed {
                        id: row.id,
                        pubkey: row.pubkey,
                        content: row.content,
                        created_at: sanitize_ts_u64(event.created_at.as_secs()),
                        kind: event.kind.as_u16() as u64,
                    })
                    .is_err()
                {
                    eprintln!("sync update channel full, dropping update");
                }
            }
        }
        Kind::EventDeletion => {
            // NIP-09 tombstone. Only the deleting author's own rows are
            // cleared — a relay replaying a stale delete must not hide a
            // re-issued post by someone else. Tombstone-wins: once flagged,
            // POST_UPSERT_SQL refuses to resurrect the row.
            let author = event.pubkey.to_hex();
            for eid in e_tags(event) {
                if let Err(e) = PostRepo::new(db).mark_deleted_in(t, &eid, &author).await {
                    eprintln!("post soft-delete: {e}");
                }
            }
        }
        _ => {
            // Kind allowlist: only kinds the app models may land in `posts`.
            // Anything else (NIP-04 DM legacy, unknown junk, hostile test
            // events) is dropped instead of being cached as a feed row.
            if POST_KIND_ALLOWLIST.contains(&event.kind.as_u16()) {
                if let Some(row) = post_row(event) {
                    ensure_author_user(db, t, event).await?;
                    PostRepo::new(db).upsert_in(t, &row).await?;
                }
            }
        }
    }
    Ok(())
}

/// Handle a batch of verified relay events inside a single `with_tx` transaction wrapper.
///
/// Returns the indices (within `events`) whose writes succeeded, so callers can
/// advance their sync cursor only for events that actually persisted — a skipped
/// event (e.g. an unparseable DM) must be refetched rather than watermark-advanced.
pub fn handle_batch(
    db: &Database,
    my_pubkey: &str,
    events: &[Event],
    tx: &tokio::sync::mpsc::Sender<SyncUpdate>,
) -> Result<Vec<usize>, DbError> {
    if events.is_empty() {
        return Ok(Vec::new());
    }

    // Pre-verify event signatures outside of the database write transaction.
    // When batch size >= 4, parallelize across CPU cores with Rayon.
    let verified_mask: Vec<bool> = if events.len() >= 4 {
        use rayon::prelude::*;
        events
            .par_iter()
            .map(soshal_nostr_core::models::verify_event)
            .collect()
    } else {
        events
            .iter()
            .map(soshal_nostr_core::models::verify_event)
            .collect()
    };

    let conn = db.conn()?;
    soshal_db_core::query::with_tx(&conn, |t| async move {
        let mut rows: Vec<PostRow> = Vec::with_capacity(events.len());
        let mut ok_pos: Vec<usize> = Vec::with_capacity(events.len());
        let mut seen_authors: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (pos, event) in events.iter().enumerate() {
            if !verified_mask[pos] {
                continue;
            }
            match event.kind {
                Kind::EncryptedDirectMessage
                | Kind::Metadata
                | Kind::ContactList
                | Kind::ZapReceipt
                | Kind::RelayList
                | Kind::Bookmarks
                | Kind::Reaction
                | Kind::Repost
                | Kind::EventDeletion => {
                    match handle_impl(db, my_pubkey, event, tx, true, &t).await {
                        Ok(()) => ok_pos.push(pos),
                        Err(e) => {
                            eprintln!("sync engine: handle kind {}: {e}", event.kind.as_u16())
                        }
                    }
                }
                _ => {
                    if POST_KIND_ALLOWLIST.contains(&event.kind.as_u16()) {
                        if let Some(row) = post_row(event) {
                            // Absorb a per-event author-ensure failure like the
                            // sibling branches above: previously the `?` aborted
                            // the whole batch transaction (rolling back every
                            // event) on one bad author write. Skip just this row.
                            let pk_hex = event.pubkey.to_hex();
                            let ensure_res = if seen_authors.contains(&pk_hex) {
                                Ok(())
                            } else {
                                ensure_author_user(db, &t, event).await
                            };
                            match ensure_res {
                                Ok(()) => {
                                    seen_authors.insert(pk_hex);
                                    if event.kind == Kind::TextNote {
                                        let author = event.pubkey.to_hex();
                                        let created_at = sanitize_ts(event.created_at.as_secs());
                                        let mut seen_tags = std::collections::HashSet::new();
                                        for tag in
                                            soshal_content_core::hashtag::extract(&event.content)
                                                .into_iter()
                                                .filter(|t| {
                                                    seen_tags.insert(t.to_ascii_lowercase())
                                                })
                                                .take(MAX_TAGS)
                                        {
                                            if let Err(e) = HashtagRepo::new(db)
                                                .upsert_in(
                                                    &t,
                                                    &HashtagRow {
                                                        tag,
                                                        pubkey: author.clone(),
                                                        last_used_at: created_at,
                                                        count: 1,
                                                    },
                                                )
                                                .await
                                            {
                                                eprintln!("sync engine: hashtag upsert: {e}");
                                            }
                                        }
                                        match maybe_notify_text_note(db, my_pubkey, &t, event, &row)
                                            .await
                                        {
                                            Ok(()) => {}
                                            Err(e) => eprintln!(
                                                "sync engine: batch text-note notify: {e}"
                                            ),
                                        }
                                    }
                                    rows.push(row);
                                    ok_pos.push(pos);
                                }
                                Err(e) => {
                                    eprintln!(
                                        "sync engine: ensure author event {}: {e}",
                                        event.id.to_hex()
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
        PostRepo::new(db).upsert_batch_in(&t, &rows).await?;
        t.commit().await?;
        for row in &rows {
            if row.kind == Kind::TextNote.as_u16() as i64
                && tx
                    .try_send(SyncUpdate::Feed {
                        id: row.id.clone(),
                        pubkey: row.pubkey.clone(),
                        content: row.content.clone(),
                        created_at: row.created_at as u64,
                        kind: row.kind as u64,
                    })
                    .is_err()
            {
                eprintln!("sync update channel full, dropping update");
            }
        }
        Ok(ok_pos)
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
        Kind::Reaction | Kind::Repost => Some(WM_FEED),
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
    fn user_row_sanitizes_media_urls() {
        let keys = Keys::generate();
        let meta = serde_json::json!({
            "name": "alice",
            "picture": "https://169.254.169.254/latest/meta-data",
            "banner": "http://localhost/banner.png",
            "about": "hi",
        })
        .to_string();
        let event = signed_event_with_tags(&keys, Kind::Metadata, &meta, vec![]);
        let row = user_row(&event).unwrap();
        assert_eq!(row.name.as_deref(), Some("alice"));
        assert!(row.picture.is_none(), "private-IP picture must be dropped");
        assert!(row.banner.is_none(), "loopback banner must be dropped");

        let good = serde_json::json!({"picture": "https://example.com/a.png"}).to_string();
        let event = signed_event_with_tags(&keys, Kind::Metadata, &good, vec![]);
        let row = user_row(&event).unwrap();
        assert_eq!(row.picture.as_deref(), Some("https://example.com/a.png"));
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
        let cur = watermark(&db, WM_FEED).max(sanitize_ts_u64(event.created_at.as_secs()));
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
            if sanitize_ts_u64(e.created_at.as_secs()) > cur {
                cur = sanitize_ts_u64(e.created_at.as_secs());
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
        // NIP-10 order: first e = root, last e = reply.
        let reply = "e1";
        let root = "e0";
        let post = signed_event_with_tags(
            &keys,
            Kind::TextNote,
            "hi",
            vec![
                vec!["e".to_string(), root.to_string()],
                vec!["e".to_string(), reply.to_string()],
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

        // Bookmarks with e-tags: rows persisted with deterministic ID format.
        let bm = signed_event_with_tags(
            &keys,
            Kind::Bookmarks,
            "",
            vec![
                vec!["e".to_string(), "evt-9".to_string()],
                vec!["e".to_string(), "evt-10".to_string()],
            ],
        );
        handle(&db, "", &bm, &tx).unwrap();
        let repo = BookmarkRepo::new(&db);
        let row9 = repo.get_by_id(&bm.id.to_hex()).unwrap().unwrap();
        assert_eq!(row9.event_id, "evt-9");
        let row10 = repo
            .get_by_id(&format!("{}:evt-10", bm.id.to_hex()))
            .unwrap()
            .unwrap();
        assert_eq!(row10.event_id, "evt-10");
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
            serde_json::json!(["a".repeat(64), "b".repeat(64)]).to_string()
        );
        assert_eq!(row.name.as_deref(), Some("alice"));

        // Re-ingest second contact list for same user: NIP-02 replace
        // semantics — the kind-3 event IS the full follow set, so the list is
        // REPLACED, not unioned, by a newer event from the same author.
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
            serde_json::json!(["c".repeat(64)]).to_string()
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

    #[test]
    fn dm_full_channel_fails_closed_watermark_and_redelivery() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let my = keys.public_key().to_hex();
        let (tx, _rx) = tokio::sync::mpsc::channel::<SyncUpdate>(1);
        // Pre-fill the single buffer slot (with the receiver held) so the
        // DM try_send below fails with Full → the position is excluded from
        // ok_pos and the watermark must not advance.
        assert!(tx
            .try_send(SyncUpdate::Profile { pubkey: my.clone() })
            .is_ok());
        set_watermark(&db, WM_DM, 1_700_000_000);

        let dm = signed_event_with_tags(
            &keys,
            Kind::EncryptedDirectMessage,
            "enc",
            vec![vec!["p".to_string(), my.clone()]],
        );

        // Full channel (pre-filled, no consumer): try_send fails → DM position
        // excluded from ok_pos → engine cursor (WM_DM) must not advance.
        let ok_pos = handle_batch(&db, &my, &[dm.clone()], &tx).unwrap();
        assert!(ok_pos.is_empty(), "undelivered DM must not be acknowledged");
        assert_eq!(watermark(&db, WM_DM), 1_700_000_000);

        // Next pass re-fetches the still-pending DM; a live channel delivers.
        let (tx2, mut rx2) = channel();
        let ok_pos = handle_batch(&db, &my, &[dm.clone()], &tx2).unwrap();
        assert_eq!(ok_pos, vec![0]);
        match rx2.try_recv().unwrap() {
            SyncUpdate::Dm { content, .. } => assert_eq!(content, "enc"),
            other => panic!("unexpected update: {other:?}"),
        }
        // Delivered → engine cursor would advance past the DM timestamp.
        let cur = watermark(&db, WM_DM).max(dm.created_at.as_secs());
        set_watermark(&db, WM_DM, cur);
        assert_eq!(watermark(&db, WM_DM), dm.created_at.as_secs());
    }

    #[test]
    fn batch_kind5_tombstones_relay_path() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let my = keys.public_key().to_hex();
        let (tx, mut rx) = channel();

        // Seed a post authored by `keys`.
        let post = soshal_test_util::signed_event(&keys, Kind::TextNote, "to delete", 100);
        handle_batch(&db, &my, &[post.clone()], &tx).unwrap();
        while rx.try_recv().is_ok() {}
        let repo = PostRepo::new(&db);
        assert_eq!(
            repo.get_by_id(&post.id.to_hex())
                .unwrap()
                .map(|r| r.is_deleted),
            Some(false)
        );

        // Kind-5 delete from the SAME author is routed through the event
        // deletion arm (previously the batch route list skipped it → the
        // tombstone never applied on the relay path).
        let del = signed_event_with_tags(
            &keys,
            Kind::EventDeletion,
            "",
            vec![vec!["e".to_string(), post.id.to_hex()]],
        );
        handle_batch(&db, &my, &[del.clone()], &tx).unwrap();
        assert_eq!(
            repo.get_by_id(&post.id.to_hex())
                .unwrap()
                .map(|r| r.is_deleted),
            Some(true),
            "kind-5 from the author must tombstone the matching post"
        );

        // A delete from a DIFFERENT author cannot hide someone else's post.
        let other = Keys::generate();
        let del_other = signed_event_with_tags(
            &other,
            Kind::EventDeletion,
            "",
            vec![vec!["e".to_string(), post.id.to_hex()]],
        );
        handle_batch(&db, &my, &[del_other], &tx).unwrap();
        assert_eq!(
            repo.get_by_id(&post.id.to_hex())
                .unwrap()
                .map(|r| r.is_deleted),
            Some(true),
            "foreign kind-5 must not resurrect or re-apply; row stays deleted"
        );
    }

    #[test]
    fn metadata_newer_wins_preserves_contact_list() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let my = keys.public_key().to_hex();
        let (tx, mut rx) = channel();

        // Seed a follow graph (kind-3) for the same author at ts=100 (older than
        // the metadata events below so ordering is deterministic).
        let other = Keys::generate().public_key().to_hex();
        let mut builder = EventBuilder::new(Kind::ContactList, r#"{"name":"ignored-metadata"}"#)
            .custom_created_at(nostr::types::Timestamp::from(100));
        builder = builder.tag(Tag::parse(vec!["p".to_string(), other.clone()]).unwrap());
        let contacts = builder.finalize(&keys).unwrap();
        handle_batch(&db, &my, &[contacts.clone()], &tx).unwrap();
        while rx.try_recv().is_ok() {}
        let row = UserRepo::new(&db).get_by_pubkey(&my).unwrap().unwrap();
        let contacts_at = |row: &soshal_db_core::repos::user::UserRow| -> Vec<String> {
            serde_json::from_str::<Vec<String>>(&row.contact_pubkeys).unwrap_or_default()
        };
        assert!(
            contacts_at(&row).contains(&other),
            "kind-3 must land in the follow graph"
        );

        // Newer metadata event (later ts): applies, but must NOT wipe the
        // merged follow graph or relay list.
        let meta_new = soshal_test_util::signed_event(
            &keys,
            Kind::Metadata,
            r#"{"name":"alice","display_name":"Alice"}"#,
            500,
        );
        handle_batch(&db, &my, &[meta_new.clone()], &tx).unwrap();
        while rx.try_recv().is_ok() {}
        let row = UserRepo::new(&db).get_by_pubkey(&my).unwrap().unwrap();
        assert_eq!(row.name.as_deref(), Some("alice"));
        assert!(
            contacts_at(&row).contains(&other),
            "metadata replay must not wipe the merged follow graph"
        );

        // STALE metadata event (older ts than the stored 500): ignored.
        let meta_old =
            soshal_test_util::signed_event(&keys, Kind::Metadata, r#"{"name":"stale-bob"}"#, 100);
        handle_batch(&db, &my, &[meta_old.clone()], &tx).unwrap();
        while rx.try_recv().is_ok() {}
        let row = UserRepo::new(&db).get_by_pubkey(&my).unwrap().unwrap();
        assert_eq!(
            row.name.as_deref(),
            Some("alice"),
            "an older metadata replay must not regress the stored profile"
        );
    }

    #[test]
    fn direct_reply_marked_root_populates_reply_to() {
        let keys = Keys::generate();
        let root = "root_event_123";
        let direct_reply = signed_event_with_tags(
            &keys,
            Kind::TextNote,
            "direct reply",
            vec![vec![
                "e".to_string(),
                root.to_string(),
                "".to_string(),
                "root".to_string(),
            ]],
        );
        let prow = post_row(&direct_reply).unwrap();
        assert_eq!(
            prow.root_id.as_deref(),
            Some(root),
            "root_id should be populated from marked root"
        );
        assert_eq!(
            prow.reply_to.as_deref(),
            Some(root),
            "reply_to should fall back to root for direct reply"
        );
    }

    #[test]
    fn batch_text_notes_indexes_hashtags() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let my = keys.public_key().to_hex();
        let (tx, _rx) = channel();

        let post = soshal_test_util::signed_event(
            &keys,
            Kind::TextNote,
            "Check out #soshal and #rust rocks! #soshal",
            100,
        );
        handle_batch(&db, &my, &[post], &tx).unwrap();

        let repo = HashtagRepo::new(&db);
        let trending = repo.get_trending(10).unwrap();
        assert_eq!(trending.len(), 2);
        let tags: Vec<&str> = trending.iter().map(|h| h.tag.as_str()).collect();
        assert!(tags.contains(&"soshal"));
        assert!(tags.contains(&"rust"));
        // Deduplicated per post: count is 1 for soshal despite being repeated
        let soshal_h = trending.iter().find(|h| h.tag == "soshal").unwrap();
        assert_eq!(soshal_h.count, 1);
    }

    #[test]
    fn zap_receipt_with_bolt11_tag_ingested() {
        let db = soshal_test_util::test_db();
        let keys = Keys::generate();
        let my = keys.public_key().to_hex();
        let (tx, _rx) = channel();

        let request = signed_event_with_tags(
            &keys,
            Kind::ZapRequest,
            "",
            vec![
                vec!["p".to_string(), my.clone()],
                vec!["e".to_string(), "note99".to_string()],
                vec!["amount".to_string(), "1000".to_string()],
            ],
        );
        handle(&db, &my, &request, &tx).unwrap();

        let canonical = zap_request_canonical_json(&post_row(&request).unwrap());
        let hash: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
        let invoice = test_invoice_with_description_hash(&hash);

        // Standard NIP-57: bolt11 invoice in tag, content is a message
        let receipt = signed_event_with_tags(
            &keys,
            Kind::ZapReceipt,
            "Thanks for the post!",
            vec![
                vec!["p".to_string(), my.clone()],
                vec!["e".to_string(), "note99".to_string()],
                vec!["bolt11".to_string(), invoice],
            ],
        );
        handle(&db, &my, &receipt, &tx).unwrap();

        let zap_count: i64 =
            query_first(&db.conn().unwrap(), "SELECT COUNT(*) FROM zaps", (), |r| {
                r.get(0)
            })
            .unwrap()
            .unwrap();
        assert_eq!(zap_count, 1, "zap with bolt11 tag should be persisted");
    }
}
