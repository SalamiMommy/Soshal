//! Feed FFI module
//!
//! Post publishing, reactions, deletion, compression, and DB-backed feed
//! queries. Publishing builds an unsigned event with nostr `EventBuilder`,
//! signs it with the unlocked signer, and relays it via the network client —
//! no key material ever passes through Dart.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::post::PostRepo;

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
}

/// Aggregate chat reactions from JSON input (delegates to feed-core).
#[frb(sync, serialize)]
pub fn feed_aggregate_chat_reactions(input: String) -> Result<String, String> {
    Ok(soshal_feed_core::reaction::aggregate_message_reactions_json(&input)).into()
}

/// Rank posts for feed display (delegates to feed-core ranking).
/// `events_json`: array of `{stats: {...}, hashtags: []}`; returns ranked
/// indices into the original array.
#[frb(serialize)]
pub async fn feed_rank_posts(events_json: String) -> Result<String, String> {
    let input: Vec<serde_json::Value> = match serde_json::from_str(&events_json) {
        Ok(v) => v,
        Err(e) => return Err(format!("invalid stats JSON: {e}")).into(),
    };
    let mut stats: Vec<soshal_feed_core::ranking::PostStats> = Vec::new();
    let mut hashtags: Vec<Vec<String>> = Vec::new();
    for item in &input {
        let st = item["stats"]
            .as_object()
            .map(|m| soshal_feed_core::ranking::PostStats {
                created_at_secs: m
                    .get("created_at_secs")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                likes_count: m.get("likes_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                replies_count: m.get("replies_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                zaps_count: m.get("zaps_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                reposts_count: m.get("reposts_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                wot_distance: m.get("wot_distance").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            });
        stats.push(st.unwrap_or(soshal_feed_core::ranking::PostStats {
            created_at_secs: 0.0,
            likes_count: 0,
            replies_count: 0,
            zaps_count: 0,
            reposts_count: 0,
            wot_distance: 0,
        }));
        hashtags.push(
            item["hashtags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        );
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

/// Validate note content (length cap, emptiness) before publishing.
#[frb(sync, serialize)]
pub fn feed_validate_note(content: String) -> Result<bool, String> {
    Ok(soshal_feed_core::publish::validate_note_content(&content).is_ok()).into()
}

/// Publish a text note (kind 1). Signs with the unlocked signer, sends to
/// relays, or queues in the persistent outbox when offline. Returns the
/// signed event JSON.
#[frb(serialize)]
pub async fn feed_publish_text_note(content: String, tags_json: String) -> Result<String, String> {
    soshal_feed_core::publish::validate_note_content(&content).map_err(|e| e.to_string())?;
    let tags: Vec<Vec<String>> =
        serde_json::from_str(&tags_json).map_err(|e| format!("invalid tags JSON: {e}"))?;
    let builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content).tags(
        tags.into_iter()
            .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let signed_json = super::signer::sign_builder(builder)?;
    if let Ok(signed) = serde_json::from_str::<serde_json::Value>(&signed_json) {
        if let (Some(id), Some(pubkey), Some(c)) = (
            signed["id"].as_str(),
            signed["pubkey"].as_str(),
            signed["content"].as_str(),
        ) {
            let _ = super::search::search_index_post(
                id.to_string(),
                pubkey.to_string(),
                c.to_string(),
                1,
            );
        }
    }
    super::sync::publish_or_enqueue("post", &signed_json).await?;
    Ok(signed_json).into()
}

/// Publish a reply (kind 1 with `e`/`p` tags referencing root/reply ids).
#[frb(serialize)]
pub async fn feed_publish_reply(
    content: String,
    root_event_id: String,
    reply_to_event_id: String,
) -> Result<String, String> {
    soshal_feed_core::publish::validate_note_content(&content).map_err(|e| e.to_string())?;
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, content);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["e".to_string(), root_event_id.clone()]) {
        builder = builder.tag(tag);
    }
    if let Ok(tag) = nostr::event::Tag::parse(vec!["e".to_string(), reply_to_event_id]) {
        builder = builder.tag(tag);
    }
    let signed_json = super::signer::sign_builder(builder)?;
    if let Ok(signed) = serde_json::from_str::<serde_json::Value>(&signed_json) {
        if let (Some(id), Some(pubkey), Some(c)) = (
            signed["id"].as_str(),
            signed["pubkey"].as_str(),
            signed["content"].as_str(),
        ) {
            let _ = super::search::search_index_post(
                id.to_string(),
                pubkey.to_string(),
                c.to_string(),
                1,
            );
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
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::Reaction, reaction_type);
    if let Ok(tag) = nostr::event::Tag::parse(vec!["e".to_string(), event_id]) {
        builder = builder.tag(tag);
    }
    let signed_json = super::signer::sign_builder(builder)?;
    super::sync::publish_or_enqueue("reaction", &signed_json).await?;
    Ok(signed_json).into()
}

/// Delete a post (kind 5 deletion request). Publishes or queues offline.
#[frb(serialize)]
pub async fn feed_delete_post(event_id: String) -> Result<String, String> {
    let mut builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::EventDeletion, "deleted by user");
    if let Ok(tag) = nostr::event::Tag::parse(vec!["e".to_string(), event_id]) {
        builder = builder.tag(tag);
    }
    let signed_json = super::signer::sign_builder(builder)?;
    super::sync::publish_or_enqueue("delete", &signed_json).await?;
    Ok(signed_json).into()
}

/// Compress event JSON for storage (zstd via content-core). Runs on the
/// Rust async runtime so the UI thread never blocks.
#[frb(serialize)]
pub async fn feed_compress_event(event_json: String) -> Result<Vec<u8>, String> {
    match soshal_content_core::compress::compress(event_json.as_bytes()) {
        Ok(bytes) => Ok(bytes).into(),
        Err(e) => Err(format!("compression failed: {e}")).into(),
    }
}

/// Decompress an event stored with `feed_compress_event`.
#[frb(serialize)]
pub async fn feed_decompress_event(compressed: Vec<u8>) -> Result<String, String> {
    match soshal_content_core::compress::decompress_limited(&compressed, 4 * 1024 * 1024) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Ok(s).into(),
            Err(e) => Err(format!("not utf8: {e}")).into(),
        },
        Err(e) => Err(format!("decompression failed: {e}")).into(),
    }
}

/// Fetch recent feed posts from the local DB (kind 1, newest first),
/// optionally filtered to posts by the given author.
#[frb(sync, serialize)]
pub fn feed_fetch_events(options_json: String) -> Result<String, String> {
    let opts: FeedOptions =
        serde_json::from_str(&options_json).map_err(|e| format!("invalid options JSON: {e}"))?;
    let limit = opts.limit.clamp(1, 200) as i64;
    super::db::with_db_result(|db| {
        let repo = PostRepo::new(db);
        let offset = opts.offset.max(0) as i64;
        let rows = repo.get_paged(limit, offset)?;
        let posts: Vec<FeedPost> = rows
            .into_iter()
            .map(|row| FeedPost {
                event_id: row.id,
                pubkey: row.pubkey,
                content: row.content,
                created_at: row.created_at.max(0) as u64,
                reactions: 0,
                replies: 0,
                reposts: 0,
                liked: false,
                media_json: soshal_feed_core::query::media_json_from_tags(&row.tags_json),
            })
            .collect();
        Ok(posts)
    })
    .map(super::util::json_ok)?
}

/// Fetch a windowed slice of feed posts from DB.
#[frb(sync, serialize)]
pub fn feed_fetch_window(start_index: u32, limit: u32) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let items =
            soshal_feed_core::window::fetch_feed_window(db, start_index as usize, limit as usize)
                .map_err(soshal_db_core::error::DbError::Migration)?;
        Ok(items)
    })
    .map(super::util::json_ok)?
}

/// Fetch a thread (root post + direct replies) from the local DB.
#[frb(sync, serialize)]
pub fn feed_fetch_thread(event_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let repo = PostRepo::new(db);
        let replies = repo.get_replies_for_root(&event_id)?;
        let out: Vec<FeedPost> = replies
            .into_iter()
            .map(|row| FeedPost {
                event_id: row.id,
                pubkey: row.pubkey,
                content: row.content,
                created_at: row.created_at.max(0) as u64,
                reactions: 0,
                replies: 0,
                reposts: 0,
                liked: false,
                media_json: soshal_feed_core::query::media_json_from_tags(&row.tags_json),
            })
            .collect();
        Ok(out)
    })
    .map(super::util::json_ok)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_note() {
        assert!(feed_validate_note("hi".to_string()).unwrap());
        assert!(!feed_validate_note("".to_string()).unwrap());
    }

    #[tokio::test]
    async fn test_compress_roundtrip() {
        let event = r#"{"id":"a","kind":1,"content":"hello world hello world"}"#;
        let compressed = feed_compress_event(event.to_string()).await.unwrap();
        let restored = feed_decompress_event(compressed).await.unwrap();
        assert_eq!(restored, event);
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
        .map_err(|e| e.to_string())
        .into()
}
