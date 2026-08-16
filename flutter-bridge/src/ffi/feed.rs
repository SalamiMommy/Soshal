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
        db::db_execute_raw(format!(
            "INSERT OR IGNORE INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('{id}','{pubkey}','{content}',{kind},{created_at},'{tags_json}','synced',0)"
        ))
        .unwrap();
    }

    #[test]
    fn test_validate_note() {
        assert!(feed_validate_note("hi".to_string()).unwrap());
        assert!(!feed_validate_note("".to_string()).unwrap());
        assert!(!feed_validate_note("x".repeat(64001)).unwrap());
        assert!(feed_validate_note("x".repeat(64000)).unwrap());
    }

    #[tokio::test]
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

    #[tokio::test]
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

    #[tokio::test]
    async fn test_publish_text_note_validation_errors() {
        let err = feed_publish_text_note(String::new(), "[]".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("content must be 1-64000 chars"), "{err}");
        let err = feed_publish_text_note("hi".to_string(), "not json".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("invalid tags JSON"), "{err}");
    }

    #[tokio::test]
    async fn test_publish_reply_validation_error() {
        let err = feed_publish_reply(String::new(), "root".to_string(), "reply".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("content must be 1-64000 chars"), "{err}");
    }

    #[tokio::test]
    async fn test_reaction_and_delete_require_signer() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::signer_lock().unwrap();
        let err = feed_create_reaction("ev1".to_string(), "👍".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("signer locked"), "{err}");
        let err = feed_delete_post("ev1".to_string()).await.unwrap_err();
        assert!(err.contains("signer locked"), "{err}");
    }

    #[test]
    fn test_fetch_events_db_paged() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("events");
        insert_post("p1", "pk1", "first", 1, 3000, "[]");
        insert_post("p2", "pk2", "second", 1, 2000, "[]");
        insert_post("p3", "pk3", "third", 1, 1000, "[]");
        let json =
            feed_fetch_events(r#"{"limit":2,"offset":0,"filter_type":"any"}"#.to_string()).unwrap();
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
            .unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["event_id"], "p3");
        let err = feed_fetch_events("bad".to_string()).unwrap_err();
        assert!(err.contains("invalid options JSON"), "{err}");
    }

    #[test]
    fn test_fetch_events_media_json() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn test_fetch_window_db() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("window");
        db::db_execute_raw(
            "INSERT INTO users (pubkey, npub, name) VALUES ('pk1','npub1pk1','tester') ON CONFLICT DO NOTHING"
                .to_string(),
        )
        .unwrap();
        insert_post("w1", "pk1", "win", 1, 2000, "[]");
        insert_post("w2", "pk2", "other", 1, 1000, "[]");
        insert_post("w3", "pk2", "reaction", 7, 3000, "[]");
        let json = feed_fetch_window(0, 2).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["event_id"], "w1");
        assert_eq!(arr[0]["profile_name"], "tester");
        assert_eq!(arr[1]["event_id"], "w2");
        let json = feed_fetch_window(1, 1).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["event_id"], "w2");
    }

    #[test]
    fn test_fetch_thread_db() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("thread");
        db::db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('root','pk1','root post',1,1000,'[]','synced',0)"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, root_id) \
             VALUES ('r1','pk2','reply one',1,2000,'[]','synced',0,'root')"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, root_id) \
             VALUES ('r2','pk2','reply two',1,1500,'[]','synced',0,'root')"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('other','pk3','unrelated',1,3000,'[]','synced',0)"
                .to_string(),
        )
        .unwrap();
        db::db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted, root_id) \
             VALUES ('rdel','pk4','deleted',1,2500,'[]','synced',1,'root')"
                .to_string(),
        )
        .unwrap();
        let json = feed_fetch_thread("root".to_string()).unwrap();
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
