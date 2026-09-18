//! Feed FFI module
//!
//! Post publishing, reactions, deletion, compression, and DB-backed feed
//! queries. Publishing builds an unsigned event with nostr `EventBuilder`,
//! signs it with the unlocked signer, and relays it via the network client —
//! no key material ever passes through Dart.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::post::{PostMetaRow, PostRepo};

use std::sync::{Mutex, OnceLock};

/// Feed post result (mirrors a DB post row for the Dart layer).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedPost {
    pub event_id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub reactions: i32,
    pub replies: i32,
    pub reposts: i32,
    pub liked: bool,
    /// JSON `{"type","url","blob_hash","size"}` when the post carries a
    /// `["media", ...]` tag (LAN blob-sharing), else `None`.
    pub media_json: Option<String>,
}

/// Feed fetch options.
#[derive(Serialize, Deserialize, Debug)]
pub struct FeedOptions {
    pub limit: i32,
    pub offset: i32,
    pub filter_type: String,
    #[serde(default)]
    pub cursor_created_at: Option<i64>,
    #[serde(default)]
    pub cursor_id: Option<String>,
    #[serde(default)]
    pub audience: String,
}

/// Aggregate chat reactions from JSON input (delegates to feed-core).
#[frb(sync, serialize)]
pub fn feed_aggregate_chat_reactions(input: String) -> Result<String, String> {
    Ok(soshal_feed_core::reaction::aggregate_message_reactions_json(&input)).into()
}

#[derive(serde::Deserialize)]
struct FeedRankItem {
    #[serde(default)]
    stats: Option<PostStatsInput>,
    #[serde(default)]
    hashtags: Option<Vec<String>>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(serde::Deserialize)]
struct PostStatsInput {
    #[serde(default)]
    created_at_secs: f64,
    #[serde(default)]
    likes_count: u64,
    #[serde(default)]
    replies_count: u64,
    #[serde(default)]
    zaps_count: u64,
    #[serde(default)]
    reposts_count: u64,
    #[serde(default)]
    wot_distance: u64,
}

/// Rank posts for feed display (delegates to feed-core ranking).
/// `events_json`: array of `{stats: {...}, hashtags: []}`; returns ranked
/// indices into the original array.
#[frb(serialize)]
pub async fn feed_rank_posts(events_json: String) -> Result<String, String> {
    if events_json.len() > 16 * 1024 * 1024 {
        return Err("events JSON too large".to_string()).into();
    }
    let input: Vec<FeedRankItem> = match serde_json::from_str(&events_json) {
        Ok(v) => v,
        Err(e) => return Err(format!("invalid stats JSON: {e}")).into(),
    };
    let mut stats: Vec<soshal_feed_core::ranking::PostStats> = Vec::with_capacity(input.len());
    let mut hashtags: Vec<Vec<String>> = Vec::with_capacity(input.len());
    for item in input {
        let st = item
            .stats
            .map(|m| soshal_feed_core::ranking::PostStats {
                created_at_secs: m.created_at_secs,
                likes_count: m.likes_count.min(u32::MAX as u64) as u32,
                replies_count: m.replies_count.min(u32::MAX as u64) as u32,
                zaps_count: m.zaps_count.min(u32::MAX as u64) as u32,
                reposts_count: m.reposts_count.min(u32::MAX as u64) as u32,
                wot_distance: m.wot_distance.min(u32::MAX as u64) as u32,
            })
            .unwrap_or(soshal_feed_core::ranking::PostStats {
                created_at_secs: 0.0,
                likes_count: 0,
                replies_count: 0,
                zaps_count: 0,
                reposts_count: 0,
                wot_distance: 0,
            });
        stats.push(st);
        let item_hashtags = if let Some(tags) = item.hashtags {
            tags
        } else if let Some(content) = item.content {
            soshal_content_core::hashtag::extract(&content)
        } else {
            Vec::new()
        };
        hashtags.push(item_hashtags);
    }
    let ranked = soshal_feed_core::ranking::rank_posts(
        &stats,
        &hashtags,
        &[],
        &soshal_feed_core::ranking::AlgoWeights::default(),
        stats.len().min(100),
        soshal_common_core::format::now_secs() as f64,
    );
    match serde_json::to_string(&ranked) {
        Ok(json) => Ok(json).into(),
        Err(e) => Err(format!("serialization failed: {e}")).into(),
    }
}

#[derive(Clone, Copy)]
struct EngagementCounters {
    reactions: i32,
    replies: i32,
    reposts: i32,
    liked: bool,
}

fn feed_engagement_counters(
    db: &soshal_db_core::Database,
    ids: &[String],
) -> std::collections::HashMap<String, EngagementCounters> {
    if ids.is_empty() {
        return std::collections::HashMap::new();
    }
    let id_json = serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string());
    let active_pubkey: String = super::db::active_pubkey()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    soshal_db_core::block_on(async {
        let r: Result<std::collections::HashMap<String, EngagementCounters>, String> = async {
            let conn = db.conn().map_err(|e| e.to_string())?;
            let stmt = conn
                .prepare(
                    "SELECT p.id,
                            (SELECT COUNT(*) FROM reactions r WHERE r.event_id = p.id),
                            (SELECT COUNT(*) FROM posts rp WHERE rp.root_id = p.id AND rp.kind = 1 AND rp.is_deleted = 0),
                            p.reposts_count,
                            EXISTS(SELECT 1 FROM reactions rl WHERE rl.event_id = p.id AND LOWER(rl.pubkey) = LOWER(?2))
                     FROM posts p
                     WHERE p.id IN (SELECT value FROM json_each(?1))",
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut out = std::collections::HashMap::new();
            let mut rows = stmt
                .query(libsql::params![id_json.as_str(), active_pubkey.as_str()])
                .await
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
                let event_id: String = row.get(0).map_err(|e| e.to_string())?;
                let reactions: i64 = row.get(1).map_err(|e| e.to_string())?;
                let replies: i64 = row.get(2).map_err(|e| e.to_string())?;
                let reposts: i64 = row.get(3).map_err(|e| e.to_string())?;
                let liked: i64 = row.get(4).map_err(|e| e.to_string())?;
                out.insert(
                    event_id,
                    EngagementCounters {
                        reactions: reactions.min(i32::MAX as i64) as i32,
                        replies: replies.min(i32::MAX as i64) as i32,
                        reposts: reposts.min(i32::MAX as i64) as i32,
                        liked: liked != 0,
                    },
                );
            }
            Ok(out)
        }
        .await;
        r.unwrap_or_default()
    })
}

fn get_custom_word_filters(db: &soshal_db_core::Database) -> Vec<String> {
    soshal_db_core::repos::settings::SettingsRepo::new(db)
        .get("moderation_word_filters")
        .ok()
        .flatten()
        .map(|raw| serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default())
        .unwrap_or_default()
}

/// Bounded verdict cache for feed moderation.
///
/// Post content is immutable after ingest, so the comprehensive check is
/// recomputed on every page fetch today. Cache keyed by content, invalidated
/// wholesale when the custom word filter list changes. Bounded: full clear
/// at the cap (eviction only costs a recompute on the next fetch).
///
/// Worst-case cap: 2048 × ~64-byte key ≈ 128 KiB of verdicts.
const MODERATION_CACHE_CAP: usize = 2048;

static MODERATION_CACHE: OnceLock<Mutex<super::util::TtlCache<String, bool>>> = OnceLock::new();
static MODERATION_FILTERS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn is_content_clean(content: &str, filters: &[String]) -> bool {
    let now = soshal_common_core::format::now_secs();
    if crate::ffi::util::lock(&MODERATION_FILTERS).as_slice() != filters {
        *crate::ffi::util::lock(&MODERATION_FILTERS) = filters.to_vec();
        crate::ffi::util::lock(MODERATION_CACHE.get_or_init(|| {
            Mutex::new(super::util::TtlCache::new(i64::MAX, MODERATION_CACHE_CAP))
        }))
        .clear();
    }
    let mut cache =
        crate::ffi::util::lock(MODERATION_CACHE.get_or_init(|| {
            Mutex::new(super::util::TtlCache::new(i64::MAX, MODERATION_CACHE_CAP))
        }));
    if let Some(&verdict) = cache.get(content, now) {
        return verdict;
    }
    let passed = soshal_moderation_core::check::check_with_custom_words(content, filters).passed;
    cache.insert(content.to_string(), passed, now);
    passed
}

/// Validate note content (length cap, emptiness, and AI moderation policy) before publishing.
#[frb(sync, serialize)]
pub fn feed_validate_note(content: String) -> Result<bool, String> {
    if soshal_feed_core::publish::validate_note_content(&content).is_err() {
        return Ok(false);
    }
    let filters =
        super::db::with_db_result(|db| Ok(get_custom_word_filters(db))).unwrap_or_default();
    Ok(is_content_clean(&content, &filters)).into()
}

/// Publish a text note (kind 1). Checks note against on-device AI moderation, signs with the
/// unlocked signer, sends to relays, or queues in the persistent outbox when offline.
#[derive(serde::Deserialize)]
struct SignedEventHeader<'a> {
    id: &'a str,
    pubkey: &'a str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct SearchIndexRow<'a> {
    id: &'a str,
    pubkey: &'a str,
    content: &'a str,
    kind: u64,
}

#[frb(serialize)]
pub async fn feed_publish_text_note(content: String, tags_json: String) -> Result<String, String> {
    soshal_feed_core::publish::validate_note_content(&content).map_err(super::util::to_err)?;
    let filters =
        super::db::with_db_result(|db| Ok(get_custom_word_filters(db))).unwrap_or_default();
    let verdict = soshal_moderation_core::check::check_with_custom_words(&content, &filters);
    if !verdict.passed {
        let reason = verdict
            .reason
            .unwrap_or_else(|| "content violated moderation policy".to_string());
        return Err(format!("content blocked by moderation filter: {reason}"));
    }
    let tags: Vec<Vec<String>> =
        serde_json::from_str(&tags_json).map_err(|e| format!("invalid tags JSON: {e}"))?;
    let builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content).tags(
        tags.into_iter()
            .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let signed_json = super::signer::sign_builder(builder)?;
    if let Ok(signed) = serde_json::from_str::<SignedEventHeader>(&signed_json) {
        if let Ok(rows) = serde_json::to_string(&[SearchIndexRow {
            id: signed.id,
            pubkey: signed.pubkey,
            content: signed.content,
            kind: 1,
        }]) {
            let _ = super::search::search_index_posts(rows);
        }
    }
    super::sync::publish_or_enqueue("post", &signed_json).await?;
    Ok(signed_json).into()
}

/// Publish a reply (kind 1 with `e`/`p` tags referencing root/reply ids).
/// Checks content with on-device AI moderation before publishing.
#[frb(serialize)]
pub async fn feed_publish_reply(
    content: String,
    root_event_id: String,
    reply_to_event_id: String,
) -> Result<String, String> {
    let root_event_id = root_event_id.trim().to_ascii_lowercase();
    let reply_to_event_id = reply_to_event_id.trim().to_ascii_lowercase();
    if root_event_id.is_empty() || root_event_id.len() > 128 {
        return Err("invalid root_event_id".to_string());
    }
    if reply_to_event_id.is_empty() || reply_to_event_id.len() > 128 {
        return Err("invalid reply_to_event_id".to_string());
    }
    soshal_feed_core::publish::validate_note_content(&content).map_err(super::util::to_err)?;
    let filters =
        super::db::with_db_result(|db| Ok(get_custom_word_filters(db))).unwrap_or_default();
    let verdict = soshal_moderation_core::check::check_with_custom_words(&content, &filters);
    if !verdict.passed {
        let reason = verdict
            .reason
            .unwrap_or_else(|| "content violated moderation policy".to_string());
        return Err(format!("reply blocked by moderation filter: {reason}"));
    }
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content);
    if let Ok(tag) = nostr::event::Tag::parse(["e", &root_event_id]) {
        builder = builder.tag(tag);
    }
    if reply_to_event_id != root_event_id {
        if let Ok(tag) = nostr::event::Tag::parse(["e", &reply_to_event_id]) {
            builder = builder.tag(tag);
        }
    }
    // NIP-10: Add 'p' tags for thread participants / authors of root and reply
    let mut author_pks: Vec<String> = Vec::new();
    let _ = super::db::with_db_result(|db| {
        let repo = PostRepo::new(db);
        if let Ok(Some(post)) = repo.get_by_id(&reply_to_event_id) {
            if !author_pks
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&post.pubkey))
            {
                author_pks.push(post.pubkey);
            }
        }
        if root_event_id != reply_to_event_id {
            if let Ok(Some(post)) = repo.get_by_id(&root_event_id) {
                if !author_pks
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&post.pubkey))
                {
                    author_pks.push(post.pubkey);
                }
            }
        }
        Ok(())
    });
    for pk in author_pks {
        if let Ok(tag) = nostr::event::Tag::parse(["p", &pk]) {
            builder = builder.tag(tag);
        }
    }
    let signed_json = super::signer::sign_builder(builder)?;
    if let Ok(signed) = serde_json::from_str::<SignedEventHeader>(&signed_json) {
        if let Ok(rows) = serde_json::to_string(&[SearchIndexRow {
            id: signed.id,
            pubkey: signed.pubkey,
            content: signed.content,
            kind: 1,
        }]) {
            let _ = super::search::search_index_posts(rows);
        }
    }
    super::sync::publish_or_enqueue("reply", &signed_json).await?;
    Ok(signed_json).into()
}

/// Create a reaction (kind 7) to an event. Publishes or queues offline.
#[frb(serialize)]
pub async fn feed_create_reaction(
    event_id: String,
    reaction_type: String,
) -> Result<String, String> {
    let event_id = event_id.trim().to_ascii_lowercase();
    if event_id.is_empty() || event_id.len() > 128 {
        return Err("invalid event_id".to_string());
    }
    // NIP-7 reactions are typically +, -, or a single emoji. 200 bytes is a
    // generous ceiling that prevents oversized events being signed and relayed.
    if reaction_type.is_empty() || reaction_type.len() > 200 {
        return Err("reaction_type must be 1–200 bytes".to_string()).into();
    }
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::Reaction, reaction_type);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["e".to_string(), event_id.clone()]) {
        builder = builder.tag(tag);
    }
    // NIP-25: Add 'p' tag pointing to the author of the event being reacted to
    let target_author: Option<String> = super::db::with_db_result(|db| {
        let repo = PostRepo::new(db);
        if let Ok(Some(post)) = repo.get_by_id(&event_id) {
            return Ok(Some(post.pubkey));
        }
        Ok(None)
    })
    .unwrap_or(None);

    if let Some(author_pk) = target_author {
        if let Ok(tag) = nostr::event::Tag::parse(["p", &author_pk]) {
            builder = builder.tag(tag);
        }
    }
    let signed_json = super::signer::sign_builder(builder)?;
    super::sync::publish_or_enqueue("reaction", &signed_json).await?;
    Ok(signed_json).into()
}

/// Delete a post (kind 5 deletion request). Publishes or queues offline.
#[frb(serialize)]
pub async fn feed_delete_post(event_id: String) -> Result<String, String> {
    let event_id = event_id.trim().to_ascii_lowercase();
    let caller = super::signer::signer_pubkey()?;
    if event_id.len() != 64 || hex::decode(&event_id).is_err() {
        return Err("invalid event_id: must be 64-character hex".to_string());
    }
    super::db::with_db_string(|db| {
        let repo = PostRepo::new(db);
        if let Ok(Some(post)) = repo.get_by_id(&event_id) {
            if !post.pubkey.eq_ignore_ascii_case(&caller) {
                return Err("cannot delete post authored by another user".to_string());
            }
            repo.delete(&event_id).map_err(|e| e.to_string())?;
        }
        Ok(())
    })?;
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::EventDeletion, "deleted by user");
    if let Ok(tag) = nostr::event::Tag::parse(vec!["e".to_string(), event_id]) {
        builder = builder.tag(tag);
    }
    let signed_json = super::signer::sign_builder(builder)?;
    super::sync::publish_or_enqueue("delete", &signed_json).await?;
    Ok(signed_json).into()
}

/// Fetch recent feed posts from the local DB (kind 1, newest first),
/// filtering out posts that trip on-device AI moderation or word filters.
#[frb(serialize)]
pub async fn feed_fetch_events(options_json: String) -> Result<String, String> {
    if options_json.len() > 1024 * 1024 {
        return Err("options JSON too large".to_string()).into();
    }
    tokio::task::spawn_blocking(move || {
        let opts: FeedOptions = serde_json::from_str(&options_json)
            .map_err(|e| format!("invalid options JSON: {e}"))?;
        let limit = opts.limit.clamp(1, 200) as i64;
        let authors: Option<Vec<String>> =
            super::identity::resolve_audience_authors(&opts.audience)?;
        super::db::with_db_result(|db| {
            let filters = get_custom_word_filters(db);
            let repo = PostRepo::new(db);
            let mut rows: Vec<PostMetaRow> = Vec::new();
            if let (Some(cursor), Some(cursor_id)) = (opts.cursor_created_at, opts.cursor_id) {
                let chunk = match authors.as_deref() {
                    Some(a) => {
                        repo.get_paged_meta_cursor_by_authors(cursor, &cursor_id, limit, a)?
                    }
                    None => repo.get_paged_meta_cursor(cursor, &cursor_id, limit)?,
                };
                rows.extend(chunk);
            } else {
                let mut offset = opts.offset.max(0) as i64;
                // Over-fetch to gather enough rows surviving the moderation
                // filter, but hard-cap iterations so a content storm of
                // blocked posts can't drive an unbounded query loop.
                let mut loops = 0;
                while rows.len() < limit as usize * 4 && loops < 32 {
                    let chunk = match authors.as_deref() {
                        Some(a) => repo.get_paged_meta_by_authors(limit, offset, a)?,
                        None => repo.get_paged_meta(limit, offset)?,
                    };
                    if chunk.is_empty() {
                        break;
                    }
                    loops += 1;
                    offset += chunk.len() as i64;
                    rows.extend(chunk);
                }
            }
            let clean_rows: Vec<PostMetaRow> = rows
                .into_iter()
                .filter(|row| is_content_clean(&row.content, &filters))
                .take(limit as usize)
                .collect();
            let ids: Vec<String> = clean_rows.iter().map(|r| r.id.clone()).collect();
            let counters = feed_engagement_counters(db, &ids);
            let posts: Vec<FeedPost> = clean_rows
                .into_iter()
                .map(|row| {
                    let ref_id = row.id.clone();
                    let content =
                        if row.content.starts_with("eNo") || row.content.starts_with("eF4") {
                            let decompressed =
                                soshal_content_core::compress::decompress_json_dict(&row.content);
                            if decompressed.is_empty() {
                                row.content
                            } else {
                                decompressed
                            }
                        } else {
                            row.content
                        };
                    let engagement = counters.get(&ref_id);
                    FeedPost {
                        event_id: ref_id,
                        pubkey: row.pubkey,
                        content,
                        created_at: row.created_at.max(0) as u64,
                        reactions: engagement.map(|c| c.reactions).unwrap_or(0),
                        replies: engagement.map(|c| c.replies).unwrap_or(0),
                        reposts: engagement.map(|c| c.reposts).unwrap_or(0),
                        liked: engagement.map(|c| c.liked).unwrap_or(false),
                        media_json: soshal_feed_core::query::media_json_from_tags(&row.tags_json),
                    }
                })
                .collect();
            Ok(posts)
        })
        .map(super::util::json_ok)?
    })
    .await
    .map_err(|e| format!("feed fetch join: {e}"))?
}

/// Fetch a windowed slice of feed posts from DB, filtering moderated items.
#[frb(serialize)]
pub async fn feed_fetch_window(
    start_index: u32,
    limit: u32,
    audience: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let authors = super::identity::resolve_audience_authors(&audience)?;
        super::db::with_db_result(|db| {
            let filters = get_custom_word_filters(db);
            let items = soshal_feed_core::window::fetch_feed_window(
                db,
                start_index as usize,
                (limit as usize * 4).clamp(1, 800),
                authors.as_deref(),
            )
            .map_err(soshal_db_core::error::DbError::Migration)?;
            let filtered_items: Vec<soshal_feed_core::window::FeedPostItem> = items
                .into_iter()
                .filter(|item| is_content_clean(&item.content, &filters))
                .take(limit as usize)
                .collect();
            Ok(filtered_items)
        })
        .map(super::util::json_ok)?
    })
    .await
    .map_err(|e| format!("feed window join: {e}"))?
}

/// Fetch a thread (root post + direct replies) from the local DB, filtering moderated replies.
#[frb(serialize)]
pub async fn feed_fetch_thread(event_id: String) -> Result<String, String> {
    let event_id = event_id.trim().to_ascii_lowercase();
    if event_id.is_empty() || event_id.len() > 128 {
        return Err("invalid event_id".to_string()).into();
    }
    tokio::task::spawn_blocking(move || {
        super::db::with_db_result(|db| {
            let filters = get_custom_word_filters(db);
            let repo = PostRepo::new(db);
            let replies = repo.get_replies_for_root(&event_id)?;
            let ids: Vec<String> = replies.iter().map(|r| r.id.clone()).collect();
            let counters = feed_engagement_counters(db, &ids);
            let out: Vec<FeedPost> = replies
                .into_iter()
                .filter(|row| is_content_clean(&row.content, &filters))
                .map(|row| {
                    let engagement = counters.get(&row.id);
                    FeedPost {
                        event_id: row.id,
                        pubkey: row.pubkey,
                        content: row.content,
                        created_at: row.created_at.max(0) as u64,
                        reactions: engagement.map(|c| c.reactions).unwrap_or(0),
                        replies: engagement.map(|c| c.replies).unwrap_or(0),
                        reposts: engagement.map(|c| c.reposts).unwrap_or(0),
                        liked: engagement.map(|c| c.liked).unwrap_or(false),
                        media_json: soshal_feed_core::query::media_json_from_tags(&row.tags_json),
                    }
                })
                .collect();
            Ok(out)
        })
        .map(super::util::json_ok)?
    })
    .await
    .map_err(|e| format!("feed thread join: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    fn tmp_db(label: &str) -> String {
        db::tmp_db(label, "feed")
    }

    fn insert_post(
        id: &str,
        pubkey: &str,
        content: &str,
        kind: i64,
        created_at: i64,
        tags_json: &str,
    ) {
        db::insert_test_user(pubkey);
        db::db_execute_params(
            "INSERT OR IGNORE INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'synced', 0)",
            &[
                id.to_string(),
                pubkey.to_string(),
                content.to_string(),
                kind.to_string(),
                created_at.to_string(),
                tags_json.to_string(),
            ],
        )
        .unwrap();
    }

    #[test]
    fn test_validate_note() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        assert!(feed_validate_note("hi".to_string()).unwrap());
        assert!(!feed_validate_note("".to_string()).unwrap());
        assert!(!feed_validate_note("x".repeat(64001)).unwrap());
        assert!(feed_validate_note("hello world ".repeat(5000)).unwrap());
        // Moderation rejection on dangerous / toxic content
        assert!(!feed_validate_note("selling cp pack".to_string()).unwrap());
        assert!(
            !feed_validate_note("watch this brutal beheading video decapitation".to_string())
                .unwrap()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_text_note_blocked_by_moderation() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("pub_mod");
        let err = feed_publish_text_note(
            "Send 1 BTC to double your crypto immediately! Guaranteed profit".to_string(),
            "[]".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("blocked by moderation filter"), "{err}");

        let err2 =
            feed_publish_text_note("selling cp pack on darknet".to_string(), "[]".to_string())
                .await
                .unwrap_err();
        assert!(err2.contains("blocked by moderation filter"), "{err2}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_reply_blocked_by_moderation() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("rep_mod");
        let err = feed_publish_reply(
            "i will find you and kill you leak your address".to_string(),
            "root".to_string(),
            "reply".to_string(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("blocked by moderation filter"), "{err}");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_fetch_events_filters_moderated_posts() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("fetch_mod");
        insert_post(
            "p_clean",
            "pk1",
            "Enjoying a lovely sunny morning!",
            1,
            3000,
            "[]",
        );
        insert_post(
            "p_spam",
            "pk2",
            "Claim free airdrop! Validate seed phrase to double crypto",
            1,
            2000,
            "[]",
        );
        insert_post("p_csam", "pk3", "selling cp pack", 1, 1000, "[]");

        let json = feed_fetch_events(r#"{"limit":10,"offset":0,"filter_type":"any"}"#.to_string())
            .await
            .unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1, "Only clean post should pass, got: {json}");
        assert_eq!(arr[0]["event_id"], "p_clean");
        assert_eq!(arr[0]["content"], "Enjoying a lovely sunny morning!");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_aggregate_chat_reactions() {
        let input = r#"{"reactions":[{"emoji":"👍","reactorPubkey":"alice"},{"emoji":"👍","reactorPubkey":"self"},{"emoji":"❤️","reactorPubkey":"bob"}],"selfPubkey":"self"}"#;
        let json = feed_aggregate_chat_reactions(input.to_string()).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["emoji"], "❤️");
        assert_eq!(arr[0]["count"], 1);
        assert_eq!(arr[0]["hasReacted"], false);
        assert_eq!(arr[1]["emoji"], "👍");
        assert_eq!(arr[1]["count"], 2);
        assert_eq!(arr[1]["hasReacted"], true);
        assert_eq!(
            feed_aggregate_chat_reactions("not json".to_string()).unwrap(),
            "[]"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rank_posts_happy_and_invalid() {
        let input = r#"[
            {"stats":{"created_at_secs":100,"likes_count":5,"replies_count":1,"zaps_count":0,"reposts_count":2,"wot_distance":0},"hashtags":["soshal"]},
            {"stats":{"created_at_secs":200,"likes_count":1,"replies_count":0,"zaps_count":0,"reposts_count":0,"wot_distance":3},"hashtags":[]}
        ]"#;
        let ranked = feed_rank_posts(input.to_string()).await.unwrap();
        let idx: Vec<usize> = serde_json::from_str(&ranked).unwrap();
        assert_eq!(idx.len(), 2, "ranked: {ranked}");
        let err = feed_rank_posts("nope".to_string()).await.unwrap_err();
        assert!(err.contains("invalid stats JSON"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_text_note_validation_errors() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("pub_val");
        let err = feed_publish_text_note(String::new(), "[]".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("content must be 1-64000 chars"), "{err}");
        let err = feed_publish_text_note("hi".to_string(), "not json".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("invalid tags JSON"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_reply_validation_error() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("rep_val");
        let err = feed_publish_reply(String::new(), "root".to_string(), "reply".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("content must be 1-64000 chars"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_reaction_and_delete_require_signer() {
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        crate::signer_lock().unwrap();
        let err = feed_create_reaction("ev1".to_string(), "👍".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("signer locked"), "{err}");
        let err = feed_delete_post("ev1".to_string()).await.unwrap_err();
        assert!(err.contains("signer locked"), "{err}");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_fetch_events_db_paged() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("events");
        insert_post("p1", "pk1", "first", 1, 3000, "[]");
        insert_post("p2", "pk2", "second", 1, 2000, "[]");
        insert_post("p3", "pk3", "third", 1, 1000, "[]");
        let json = feed_fetch_events(r#"{"limit":2,"offset":0,"filter_type":"any"}"#.to_string())
            .await
            .unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["event_id"], "p1");
        assert_eq!(arr[0]["pubkey"], "pk1");
        assert_eq!(arr[0]["content"], "first");
        assert_eq!(arr[0]["created_at"], 3000);
        assert_eq!(arr[0]["reactions"], 0);
        assert_eq!(arr[0]["liked"], false);
        let json = feed_fetch_events(r#"{"limit":10,"offset":2,"filter_type":"any"}"#.to_string())
            .await
            .unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["event_id"], "p3");
        let err = feed_fetch_events("bad".to_string()).await.unwrap_err();
        assert!(err.contains("invalid options JSON"), "{err}");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_fetch_events_media_json() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("media");
        let hash = "a".repeat(64);
        let tags = format!(r#"[["media","image","blob://{hash}","{hash}","123"]]"#);
        insert_post("pm", "pk1", "pic", 1, 1000, &tags);
        insert_post(
            "pbad",
            "pk1",
            "bad",
            1,
            900,
            r#"[["media","video","x","short","1"]]"#,
        );
        let json = feed_fetch_events(r#"{"limit":10,"offset":0,"filter_type":"any"}"#.to_string())
            .await
            .unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        let media = arr[0]["media_json"].as_str().unwrap();
        let mv: serde_json::Value = serde_json::from_str(media).unwrap();
        assert_eq!(mv["type"], "image");
        assert_eq!(mv["blob_hash"], hash);
        assert_eq!(mv["size"], 123);
        assert!(arr[1]["media_json"].is_null());
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_fetch_window_db() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("window");
        db::db_execute_raw_test(
            "INSERT INTO users (pubkey, npub, name) VALUES ('pk1','npub1pk1','tester') ON CONFLICT DO NOTHING"
                .to_string(),
        )
        .unwrap();
        insert_post("w1", "pk1", "win", 1, 2000, "[]");
        insert_post("w2", "pk2", "other", 1, 1000, "[]");
        insert_post("w3", "pk2", "reaction", 7, 3000, "[]");
        let json = feed_fetch_window(0, 2, String::new()).await.unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["event_id"], "w1");
        assert_eq!(arr[0]["profile_name"], "tester");
        assert_eq!(arr[1]["event_id"], "w2");
        let json = feed_fetch_window(1, 1, String::new()).await.unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["event_id"], "w2");
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_fetch_thread_db() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("thread");
        db::insert_test_user("pk1");
        db::insert_test_user("pk2");
        db::insert_test_user("pk3");
        db::insert_test_user("pk4");
        db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('root','pk1','root post',1,1000,'[]','synced',0)"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, root_id) \
             VALUES ('r1','pk2','reply one',1,2000,'[]','synced',0,'root')"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, root_id) \
             VALUES ('r2','pk2','reply two',1,1500,'[]','synced',0,'root')"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('other','pk3','unrelated',1,3000,'[]','synced',0)"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw_test(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, root_id) \
             VALUES ('rdel','pk4','deleted',1,2500,'[]','synced',1,'root')"
                .to_string(),
        )
        .unwrap();
        let json = feed_fetch_thread("root".to_string()).await.unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 3, "json: {json}");
        assert_eq!(arr[0]["event_id"], "root");
        assert_eq!(arr[1]["event_id"], "r2");
        assert_eq!(arr[2]["event_id"], "r1");
    }

    #[test]
    fn test_compute_card_layout() {
        let req = r#"{"id":"c1","text":{"content":"hello","font_size_px":14,"line_height_factor":1.2,"max_width_px":300,"bold":false,"max_lines":3},"media":[{"w":100,"h":50}]}"#;
        let json = feed_compute_card_layout(req.to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], "c1");
        assert!(v["height_px"].as_f64().unwrap() > 0.0);
        assert_eq!(v["media"].as_array().unwrap().len(), 1);
        assert!(feed_compute_card_layout("nope".to_string()).is_err());
    }

    #[test]
    fn test_compute_card_layouts_batch() {
        let reqs = r#"[{"id":"a","text":{"content":"x","font_size_px":12,"line_height_factor":1.2,"max_width_px":200,"bold":false,"max_lines":2},"media":[]},{"id":"b","media":[]}]"#;
        let json = feed_compute_card_layouts(reqs.to_string()).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["id"], "a");
        assert_eq!(arr[1]["id"], "b");
        assert!(feed_compute_card_layouts("nope".to_string()).is_err());
    }

    #[test]
    fn test_create_reaction_validates_reaction_type() {
        // Empty reaction_type must be rejected.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(feed_create_reaction("a".repeat(64), "".to_string()))
            .unwrap_err();
        assert!(err.contains("reaction_type"), "got: {err}");

        // reaction_type exceeding 200 bytes must be rejected.
        let err = rt
            .block_on(feed_create_reaction("a".repeat(64), "x".repeat(201)))
            .unwrap_err();
        assert!(err.contains("200"), "got: {err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_delete_post_case_insensitive_author() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("del_case");
        let keys = soshal_nostr_core::keys::generate_keys();
        crate::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let my_pk = keys.public_key().to_hex();
        let ev_id = "f".repeat(64);
        insert_post(
            &ev_id,
            &my_pk.to_ascii_uppercase(),
            "delete me",
            1,
            1000,
            "[]",
        );
        let res = feed_delete_post(ev_id).await;
        assert!(res.is_ok(), "error: {:?}", res);
        crate::signer_lock().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_feed_payload_size_limits() {
        let huge_rank = "0".repeat(17 * 1024 * 1024);
        assert!(feed_rank_posts(huge_rank).await.is_err());

        let huge_opts = "0".repeat(2 * 1024 * 1024);
        assert!(feed_fetch_events(huge_opts).await.is_err());

        assert!(feed_fetch_thread("".to_string()).await.is_err());
        assert!(feed_fetch_thread("x".repeat(129)).await.is_err());

        let huge_req = "0".repeat(2 * 1024 * 1024);
        assert!(feed_compute_card_layout(huge_req).is_err());
    }
}

/// Pre-calculated layout extents for one feed card. Request:
/// {"id","text":{"content","font_size_px","line_height_factor",
/// "max_width_px","bold","max_lines"},"media":[{"w","h"}],"chrome":{...}}
/// Returns {"id","height_px","text":{"lines","height_px","last_line_width_px",
/// "elided","max_width_px"},"media":[{"height_px","width_px"}],
/// "media_height_px"}.
#[frb(sync, serialize)]
pub fn feed_compute_card_layout(request_json: String) -> Result<String, String> {
    if request_json.len() > 1024 * 1024 {
        return Err("request JSON too large".to_string()).into();
    }
    let result = soshal_layout_core::compute_card_layout_json(&request_json);
    if result.is_empty() {
        Err("invalid layout request".to_string()).into()
    } else {
        Ok(result).into()
    }
}

/// Batch variant: requests_json = array of card requests; result = array of
/// layout results, in the same order (stable for ListView.builder maps).
#[frb(sync, serialize)]
pub fn feed_compute_card_layouts(requests_json: String) -> Result<String, String> {
    if requests_json.len() > 8 * 1024 * 1024 {
        return Err("requests JSON too large".to_string()).into();
    }
    let requests: Vec<soshal_layout_core::CardLayoutRequest> =
        match serde_json::from_str(&requests_json) {
            Ok(v) => v,
            Err(e) => return Err(format!("invalid layout batch: {e}")).into(),
        };
    let mut results = Vec::with_capacity(requests.len());
    for req in &requests {
        results.push(soshal_layout_core::compute_card_layout(req));
    }
    serde_json::to_string(&results)
        .map_err(super::util::to_err)
        .into()
}
