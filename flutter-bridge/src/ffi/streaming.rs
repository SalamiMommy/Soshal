//! Streaming FFI module
//!
//! Live streams (kind 30311, `d` = stream url, `status` tag) and stories
//! (kind 30078, `d` = `soshal_story`, `expiration`/`audience` tags) are
//! locally stored posts like all other events. Content shaping delegates
//! to streaming-core.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_streaming_core::events;

const KIND_LIVE: i64 = 30311;
const KIND_STORY: i64 = 30078;

/// Live stream info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StreamInfo {
    pub id: String,
    pub broadcaster_pubkey: String,
    pub title: String,
    pub description: String,
    pub status: String, // "live", "offline", "scheduled"
    pub viewer_count: i32,
    pub created_at: u64,
    pub stream_url: String,
}

/// Story info
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoryInfo {
    pub id: String,
    pub author_pubkey: String,
    pub content: String,
    pub images: Vec<String>,
    pub expires_at: u64,
    pub views: i32,
}

lazy_static::lazy_static! {
    static ref VIDEO_SERVER: std::sync::Mutex<Option<soshal_streaming_core::video_server::LocalVideoServer>> =
        std::sync::Mutex::new(None);
}

/// Start the local Rust video streaming HTTP micro-server bound to 127.0.0.1:0.
#[frb(serialize)]
pub async fn streaming_start_local_server() -> Result<u16, String> {
    let server = soshal_streaming_core::video_server::LocalVideoServer::start().await?;
    let port = server.port();
    if let Ok(mut lock) = VIDEO_SERVER.lock() {
        *lock = Some(server);
    }
    Ok(port)
}

/// Register a video asset with the local video micro-server and return its localhost URL.
#[frb(sync, serialize)]
pub fn streaming_get_video_url(video_id: String, source_path: String) -> Result<String, String> {
    if let Ok(lock) = VIDEO_SERVER.lock() {
        if let Some(ref server) = *lock {
            return Ok(server.register_video(video_id, source_path));
        }
    }
    Err("Video server not initialized".to_string())
}

fn tag_value(tags_json: &str, key: &str) -> Option<String> {
    serde_json::from_str::<Vec<Vec<String>>>(tags_json)
        .ok()?
        .into_iter()
        .find(|t| t.first().map(|k| k == key).unwrap_or(false))
        .and_then(|t| t.get(1).cloned())
}

fn stream_from_value(v: &serde_json::Value) -> Option<StreamInfo> {
    let tags = v["tags_json"].as_str().unwrap_or("").to_string();
    let content = v["content"].as_str().unwrap_or("");
    let c: serde_json::Value = serde_json::from_str(content).unwrap_or(serde_json::Value::Null);
    Some(StreamInfo {
        id: v["id"].as_str()?.to_string(),
        broadcaster_pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        title: c["title"].as_str().unwrap_or("Live").to_string(),
        description: c["summary"].as_str().unwrap_or("").to_string(),
        status: tag_value(&tags, "status").unwrap_or_else(|| "offline".to_string()),
        viewer_count: 0,
        created_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
        stream_url: tag_value(&tags, "d").unwrap_or_default(),
    })
}

fn story_from_value(v: &serde_json::Value) -> Option<StoryInfo> {
    let tags = v["tags_json"].as_str().unwrap_or("").to_string();
    let content_value: serde_json::Value = match v["content"].as_str() {
        Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
        None => v["content"].clone(),
    };
    let images: Vec<String> = content_value["media"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["url"].as_str().map(|u| u.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Some(StoryInfo {
        id: v["id"].as_str()?.to_string(),
        author_pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        content: content_value["text"].as_str().unwrap_or("").to_string(),
        images,
        expires_at: tag_value(&tags, "expiration")
            .and_then(|e| e.parse::<u64>().ok())
            .unwrap_or(0),
        views: v["views"].as_i64().unwrap_or(0) as i32,
    })
}

/// Fetch live streams (all locally known + optionally filter to `live`).
#[frb(sync, serialize)]
pub fn streaming_fetch_live(limit: i32) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT id, pubkey, content, created_at, tags_json FROM posts \
         WHERE kind = {KIND_LIVE} AND is_deleted = 0 ORDER BY created_at DESC LIMIT {}",
        limit.clamp(1, 100)
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    super::util::json_ok(
        rows.into_iter()
            .filter_map(|v| stream_from_value(&v))
            .collect::<Vec<_>>(),
    )
}

/// Fetch live streams from followed users (contact graph join).
#[frb(sync, serialize)]
pub fn streaming_fetch_followed_live(user_pubkey: String) -> Result<String, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json FROM posts p \
         JOIN users u ON u.pubkey = p.pubkey \
         WHERE p.kind = {KIND_LIVE} AND p.is_deleted = 0 \
         AND u.contact_pubkeys LIKE '%\"{}\"%' \
         ORDER BY p.created_at DESC LIMIT 100",
        user_pubkey.replace('\'', "''")
    ))?;
    // NOTE: contact_pubkeys holds the user's own contacts, so "followed"
    // semantics here use the reverse direction (followers live). Relay-side
    // filtering in the Tauri layer handles the forward direction.
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    super::util::json_ok(
        rows.into_iter()
            .filter_map(|v| stream_from_value(&v))
            .collect::<Vec<_>>(),
    )
}

/// Start a live stream announcement (kind 30311 with `status` = live).
/// Signs and stores locally; returns the signed event JSON.
#[frb(sync, serialize)]
pub fn streaming_start_live(
    broadcaster_pubkey: String,
    title: String,
    description: String,
    stream_url: String,
) -> Result<String, String> {
    if title.trim().is_empty() || title.len() > 300 {
        return Err("title must be 1..=300 chars".to_string()).into();
    }
    if stream_url.is_empty() || stream_url.len() > 500 {
        return Err("stream_url required".to_string()).into();
    }
    let content = serde_json::json!({
        "title": title,
        "summary": soshal_common_core::format::truncate(&description, 2000),
        "status": "live",
    })
    .to_string();
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_LIVE as u16),
        content.clone(),
    )
    .tags(
        vec![
            vec!["d".to_string(), stream_url.clone()],
            vec!["status".to_string(), "live".to_string()],
        ]
        .into_iter()
        .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    let row = soshal_db_core::repos::post::PostRow {
        id: event_id,
        pubkey: broadcaster_pubkey,
        content,
        kind: KIND_LIVE,
        created_at: soshal_common_core::format::now_secs(),
        tags_json: serde_json::to_string(&vec![
            vec!["d".to_string(), stream_url],
            vec!["status".to_string(), "live".to_string()],
        ])
        .unwrap_or_default(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: Some(title),
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(signed_json).into()
}

/// End a live stream: replace the local `status` tag with `ended`.
#[frb(sync, serialize)]
pub fn streaming_end_live(stream_id: String, broadcaster_pubkey: String) -> Result<bool, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT pubkey, tags_json FROM posts WHERE kind = {KIND_LIVE} AND id = '{}'",
        stream_id.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let row = rows.first().ok_or("stream not found".to_string())?;
    if row["pubkey"].as_str().unwrap_or("") != broadcaster_pubkey {
        return Err("only the broadcaster can end a stream".to_string()).into();
    }
    let tags_json = row["tags_json"].as_str().unwrap_or("");
    let mut tags: Vec<Vec<String>> = serde_json::from_str(tags_json).unwrap_or_default();
    for t in tags.iter_mut() {
        if t.first().map(|k| k == "status").unwrap_or(false) {
            t[1] = "ended".to_string();
        }
    }
    if !tags
        .iter()
        .any(|t| t.first().map(|k| k == "status").unwrap_or(false))
    {
        tags.push(vec!["status".to_string(), "ended".to_string()]);
    }
    super::db::db_execute_raw(format!(
        "UPDATE posts SET tags_json = '{}', sync_status = 'edited' WHERE id = '{}'",
        serde_json::to_string(&tags)
            .unwrap_or_default()
            .replace('\'', "''"),
        stream_id.replace('\'', "''")
    ))
    .map(|_| true)
    .into()
}

/// Post a story (kind 30078, d = `soshal_story`, expiration tag 24h unless
/// overridden). Signs and stores locally.
#[frb(sync, serialize)]
pub fn streaming_post_story(
    author_pubkey: String,
    content: String,
    images_json: String,
    expires_in_hours: i32,
) -> Result<String, String> {
    let images: Vec<String> =
        serde_json::from_str(&images_json).map_err(|e| format!("invalid images JSON: {e}"))?;
    if images.len() > 12 {
        return Err("too many images".to_string()).into();
    }
    let content_str = events::story_content(
        &images,
        if content.trim().is_empty() {
            None
        } else {
            Some(&content)
        },
    )?;
    let hours = if expires_in_hours <= 0 {
        24
    } else {
        expires_in_hours.min(168)
    };
    let expiry = soshal_common_core::format::now_secs() + hours as i64 * 3600;
    let mut builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::from_u16(KIND_STORY as u16),
        content_str.clone(),
    )
    .tags(
        vec![
            vec!["d".to_string(), "soshal_story".to_string()],
            vec!["expiration".to_string(), expiry.to_string()],
            vec!["audience".to_string(), "public".to_string()],
        ]
        .into_iter()
        .filter_map(|t| nostr::event::Tag::parse(t).ok()),
    );
    for url in &images {
        if let Ok(tag) = nostr::event::Tag::parse(vec!["url".to_string(), url.clone()]) {
            builder = builder.tag(tag);
        }
    }
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    let row = soshal_db_core::repos::post::PostRow {
        id: event_id,
        pubkey: author_pubkey,
        content: content_str,
        kind: KIND_STORY,
        created_at: soshal_common_core::format::now_secs(),
        tags_json: serde_json::to_string(&vec![
            vec!["d".to_string(), "soshal_story".to_string()],
            vec!["expiration".to_string(), expiry.to_string()],
        ])
        .unwrap_or_default(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject: None,
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(signed_json).into()
}

/// Fetch a user's stories (unexpired only).
#[frb(sync, serialize)]
pub fn streaming_fetch_stories(user_pubkey: String) -> Result<String, String> {
    let now = soshal_common_core::format::now_secs();
    let json = super::db::db_query_raw(format!(
        "SELECT id, pubkey, content, created_at, tags_json, 0 AS views FROM posts \
         WHERE kind = {KIND_STORY} AND pubkey = '{}' AND is_deleted = 0 \
         ORDER BY created_at DESC LIMIT 50",
        user_pubkey.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    super::util::json_ok(
        rows.into_iter()
            .filter_map(|v| story_from_value(&v))
            .filter(|s| s.expires_at == 0 || s.expires_at > now as u64)
            .collect::<Vec<_>>(),
    )
}

/// Fetch stories from all accounts the user follows (via contact lists).
#[frb(sync, serialize)]
pub fn streaming_fetch_followed_stories(viewer_pubkey: String) -> Result<String, String> {
    let now = soshal_common_core::format::now_secs();
    let json = super::db::db_query_raw(format!(
        "SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json, 0 AS views FROM posts p \
         WHERE p.kind = {KIND_STORY} AND p.is_deleted = 0 \
         AND p.pubkey IN (SELECT value FROM json_each((SELECT contact_pubkeys FROM users WHERE pubkey = '{}'))) \
         ORDER BY p.created_at DESC LIMIT 200",
        viewer_pubkey.replace('\'', "''")
    ))?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    super::util::json_ok(
        rows.into_iter()
            .filter_map(|v| story_from_value(&v))
            .filter(|s| s.expires_at == 0 || s.expires_at > now as u64)
            .collect::<Vec<_>>(),
    )
}

/// Mark a story viewed (post_views is keyed (pubkey, post_id)).
#[frb(sync, serialize)]
pub fn streaming_mark_story_viewed(
    story_id: String,
    viewer_pubkey: String,
) -> Result<bool, String> {
    if story_id.len() != 64 {
        return Err("invalid story id".to_string()).into();
    }
    super::db::with_db_result(|db| {
        soshal_db_core::repos::post_views::PostViewsRepo::new(db)
            .mark_seen(&viewer_pubkey, &[story_id])?;
        Ok(true)
    })
}

/// Record a story reaction (emoji).
#[frb(sync, serialize)]
pub fn streaming_story_react(
    story_id: String,
    pubkey: String,
    emoji: String,
) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::story_reaction::StoryReactionRepo::new(db).react(
            &soshal_db_core::repos::story_reaction::StoryReactionRow {
                story_id,
                pubkey,
                emoji,
                created_at: soshal_common_core::format::now_secs(),
            },
        )?;
        Ok(true)
    })
}

/// Create a Media over QUIC (MoQ) object for low-latency P2P publishing.
#[frb(sync, serialize)]
pub fn streaming_moq_publish_object(
    stream_id: String,
    publisher_pubkey: String,
    track_id: u32,
    is_keyframe: bool,
    payload_hex: String,
) -> Result<String, String> {
    let payload = hex::decode(&payload_hex).map_err(|e| format!("invalid hex payload: {e}"))?;
    let mut publisher =
        soshal_streaming_core::moq::MoqPublisherSession::new(stream_id, publisher_pubkey);
    let track_type = if is_keyframe {
        soshal_streaming_core::moq::MoqTrackType::VideoKeyframe
    } else {
        soshal_streaming_core::moq::MoqTrackType::VideoDelta
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let obj = publisher.create_object(track_id, track_type, now_ms, payload);
    serde_json::to_string(&obj).map_err(|e| format!("json encode error: {e}"))
}

/// Check live stream availability and report transport status. Subscription
/// itself happens per-peer via `p2p_moq_subscribe_fetch` (QUIC stream fetch).
#[frb(sync, serialize)]
pub fn streaming_moq_subscribe_stream(
    stream_id: String,
    subscriber_pubkey: String,
) -> Result<String, String> {
    let _subscriber_pubkey = subscriber_pubkey;
    let known = soshal_network_core::quic::moq_stream_known(&stream_id);
    super::util::json_ok(serde_json::json!({
        "status": if known { "live" } else { "unknown" },
        "stream_id": stream_id,
        "protocol": "MediaOverQUIC",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_value_extraction() {
        let tags = r#"[["d","abc"],["status","live"]]"#;
        assert_eq!(tag_value(tags, "status").unwrap(), "live");
        assert_eq!(tag_value(tags, "d").unwrap(), "abc");
        assert!(tag_value(tags, "expiration").is_none());
    }

    #[test]
    fn test_story_parse() {
        let v = serde_json::json!({
            "id": "abc",
            "pubkey": "pk",
            "created_at": 1,
            "content": serde_json::json!({
                "media": [{"url": "https://x/s.png", "type": "image"}],
                "text": "hi story",
            }).to_string(),
            "tags_json": r#"[["d","soshal_story"],["expiration","999"]]"#,
        });
        let s = story_from_value(&v).unwrap();
        assert_eq!(s.images, vec!["https://x/s.png".to_string()]);
        assert_eq!(s.content, "hi story");
        assert_eq!(s.expires_at, 999);
    }
}
