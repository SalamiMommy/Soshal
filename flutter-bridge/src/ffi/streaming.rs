//! Streaming FFI module
//!
//! Live streams (kind 30311, `d` = stream url, `status` tag) and stories
//! (kind 30078, `d` = `soshal_story`, `expiration`/`audience` tags) are
//! locally stored posts like all other events. Content shaping delegates
//! to streaming-core.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_common_core::consts::{KIND_LIVE, KIND_STORY};
use soshal_streaming_core::events;

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

fn tag_value(tags_json: &str, key: &str) -> Option<String> {
    if tags_json.is_empty() || tags_json == "[]" {
        return None;
    }
    serde_json::from_str::<Vec<Vec<String>>>(tags_json)
        .ok()?
        .into_iter()
        .find(|t| t.first().map(|k| k == key).unwrap_or(false))
        .and_then(|mut t| {
            if t.len() > 1 {
                Some(t.swap_remove(1))
            } else {
                None
            }
        })
}

/// Set the `status` tag value to `ended` in place. Hostile relay data can
/// carry a 1-element `["status"]` tag — never index `[1]` unchecked.
fn mark_status_ended(tags: &mut [Vec<String>]) {
    for t in tags.iter_mut() {
        if t.first().map(|k| k == "status").unwrap_or(false) {
            if let Some(v) = t.get_mut(1) {
                *v = "ended".to_string();
            }
        }
    }
}

fn stream_from_fields(
    id: String,
    pubkey: String,
    content_str: &str,
    created_at: i64,
    tags_json: &str,
) -> StreamInfo {
    let parsed_tags: Vec<Vec<String>> = serde_json::from_str(tags_json).unwrap_or_default();
    let mut status: Option<String> = None;
    let mut stream_url: Option<String> = None;
    for t in &parsed_tags {
        if let Some(key) = t.first().map(|s| s.as_str()) {
            match key {
                "status" if status.is_none() => {
                    status = t.get(1).cloned();
                }
                "d" if stream_url.is_none() => {
                    stream_url = t.get(1).cloned();
                }
                _ => {}
            }
        }
        if status.is_some() && stream_url.is_some() {
            break;
        }
    }
    let c: serde_json::Value = serde_json::from_str(content_str).unwrap_or(serde_json::Value::Null);
    StreamInfo {
        id,
        broadcaster_pubkey: pubkey,
        title: c["title"].as_str().unwrap_or("Live").to_string(),
        description: c["summary"].as_str().unwrap_or("").to_string(),
        status: status.unwrap_or_else(|| "offline".to_string()),
        viewer_count: 0,
        created_at: created_at.max(0) as u64,
        stream_url: stream_url.unwrap_or_default(),
    }
}

fn stream_from_value(v: &serde_json::Value) -> Option<StreamInfo> {
    let tags_str = v["tags_json"].as_str().unwrap_or("");
    let parsed_tags: Vec<Vec<String>> = serde_json::from_str(tags_str).unwrap_or_default();
    let mut status: Option<String> = None;
    let mut stream_url: Option<String> = None;
    for t in &parsed_tags {
        if let Some(key) = t.first().map(|s| s.as_str()) {
            match key {
                "status" if status.is_none() => {
                    status = t.get(1).cloned();
                }
                "d" if stream_url.is_none() => {
                    stream_url = t.get(1).cloned();
                }
                _ => {}
            }
        }
        if status.is_some() && stream_url.is_some() {
            break;
        }
    }
    let content = v["content"].as_str().unwrap_or("");
    let c: serde_json::Value = serde_json::from_str(content).unwrap_or(serde_json::Value::Null);
    Some(StreamInfo {
        id: v["id"].as_str()?.to_string(),
        broadcaster_pubkey: v["pubkey"].as_str().unwrap_or("").to_string(),
        title: c["title"].as_str().unwrap_or("Live").to_string(),
        description: c["summary"].as_str().unwrap_or("").to_string(),
        status: status.unwrap_or_else(|| "offline".to_string()),
        viewer_count: 0,
        created_at: v["created_at"].as_i64().unwrap_or(0).max(0) as u64,
        stream_url: stream_url.unwrap_or_default(),
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
            .map(|e| e.parse::<u64>().unwrap_or(1))
            .unwrap_or(0),
        views: v["views"].as_i64().unwrap_or(0) as i32,
    })
}

/// Fetch live streams (all locally known + optionally filter to `live`).
#[frb(sync, serialize)]
pub fn streaming_fetch_live(limit: i32, audience: String) -> Result<String, String> {
    let limit_clamped = limit.clamp(1, 100) as i64;
    let authors = super::identity::resolve_audience_authors(&audience)?;
    if let Some(a) = &authors {
        if a.is_empty() {
            return super::util::json_ok(Vec::<serde_json::Value>::new());
        }
    }
    let authors_json = authors
        .as_ref()
        .map(|a| serde_json::to_string(a).map_err(|e| format!("authors: {e}")))
        .transpose()?;
    let rows = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let sql = if authors_json.is_some() {
            "SELECT id, pubkey, content, created_at, tags_json FROM posts \
             WHERE kind = ?1 AND is_deleted = 0 \
             AND pubkey IN (SELECT value FROM json_each(?3)) \
             ORDER BY created_at DESC LIMIT ?2"
        } else {
            "SELECT id, pubkey, content, created_at, tags_json FROM posts \
             WHERE kind = ?1 AND is_deleted = 0 ORDER BY created_at DESC LIMIT ?2"
        };
        let mut bind: Vec<libsql::Value> = vec![
            libsql::Value::Integer(KIND_LIVE as i64),
            libsql::Value::Integer(limit_clamped),
        ];
        if let Some(j) = &authors_json {
            bind.push(libsql::Value::Text(j.clone()));
        }
        let rows = soshal_db_core::query::query(&conn, sql, libsql::params_from_iter(bind), |r| {
            let id: String = r.get(0)?;
            let pubkey: String = r.get(1)?;
            let content: String = r.get(2)?;
            let created_at: i64 = r.get(3)?;
            let tags_json: String = r.get(4)?;
            Ok(serde_json::json!({
                "id": id,
                "pubkey": pubkey,
                "content": content,
                "created_at": created_at,
                "tags_json": tags_json,
            }))
        })?;
        Ok(rows)
    })?;
    super::util::json_ok(
        rows.into_iter()
            .filter_map(|v| stream_from_value(&v))
            .collect::<Vec<_>>(),
    )
}

/// Fetch live streams from followed users (contact graph join).
#[frb(sync, serialize)]
pub fn streaming_fetch_followed_live(user_pubkey: String) -> Result<String, String> {
    let streams: Vec<StreamInfo> = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let sql = "SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json FROM posts p \
                   WHERE p.kind = ?1 AND p.is_deleted = 0 \
                   AND p.pubkey IN (SELECT value FROM json_each((SELECT contact_pubkeys FROM users WHERE pubkey = ?2))) \
                   ORDER BY p.created_at DESC LIMIT 100";
        let rows = soshal_db_core::query::query(
            &conn,
            sql,
            libsql::params![KIND_LIVE as i64, user_pubkey.as_str()],
            |r| {
                let id: String = r.get(0)?;
                let pubkey: String = r.get(1)?;
                let content: String = r.get(2)?;
                let created_at: i64 = r.get(3)?;
                let tags_json: String = r.get(4)?;
                Ok(stream_from_fields(
                    id, pubkey, &content, created_at, &tags_json,
                ))
            },
        )?;
        Ok(rows)
    })?;
    super::util::json_ok(streams)
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
    super::signer::require_identity(&broadcaster_pubkey)?;
    if title.trim().is_empty() || title.len() > 300 {
        return Err("title must be 1..=300 chars".to_string()).into();
    }
    if stream_url.is_empty() || stream_url.len() > 500 {
        return Err("stream_url must be 1..=500 chars".to_string()).into();
    }
    let content = serde_json::json!({
        "title": title,
        "summary": description,
    })
    .to_string();
    let builder =
        nostr::event::EventBuilder::new(nostr::event::Kind::from_u16(KIND_LIVE), content.clone())
            .tag(nostr::event::Tag::parse(vec!["d".to_string(), stream_url.clone()]).unwrap())
            .tag(nostr::event::Tag::parse(vec!["status".to_string(), "live".to_string()]).unwrap());
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("bad signed event: {e}"))?;
    let event_id = signed["id"].as_str().unwrap_or_default().to_string();
    super::db::upsert_post_row(
        event_id,
        broadcaster_pubkey,
        content,
        KIND_LIVE as i64,
        soshal_common_core::format::now_secs(),
        serde_json::to_string(&vec![
            vec!["d".to_string(), stream_url],
            vec!["status".to_string(), "live".to_string()],
        ])
        .unwrap_or_default(),
        Some(title),
    )?;
    Ok(signed_json).into()
}

/// End a live stream: replace the local `status` tag with `ended`.
#[frb(sync, serialize)]
pub fn streaming_end_live(stream_id: String, broadcaster_pubkey: String) -> Result<bool, String> {
    super::signer::require_identity(&broadcaster_pubkey)?;
    let (row_pubkey, tags_json): (String, String) = super::db::with_db_string(|db| {
        let conn = db.conn().map_err(|e| e.to_string())?;
        let res = soshal_db_core::query::query_first(
            &conn,
            "SELECT pubkey, tags_json FROM posts WHERE kind = ?1 AND id = ?2",
            libsql::params![KIND_LIVE as i64, stream_id.as_str()],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )
        .map_err(|e| e.to_string())?;
        res.ok_or_else(|| "stream not found".to_string())
    })?;
    if row_pubkey != broadcaster_pubkey {
        return Err("only the broadcaster can end a stream".to_string()).into();
    }
    let mut tags: Vec<Vec<String>> = serde_json::from_str(&tags_json).unwrap_or_default();
    mark_status_ended(&mut tags);
    if !tags
        .iter()
        .any(|t| t.first().map(|k| k == "status").unwrap_or(false))
    {
        tags.push(vec!["status".to_string(), "ended".to_string()]);
    }
    let tags_json = serde_json::to_string(&tags).unwrap_or_default();
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::block_on(conn.execute(
            "UPDATE posts SET tags_json = ?1, sync_status = 'edited' WHERE id = ?2",
            libsql::params![tags_json.as_str(), stream_id.as_str()],
        ))
        .map_err(soshal_db_core::error::DbError::from)?;
        Ok(())
    })?;
    Ok(true).into()
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
    super::signer::require_identity(&author_pubkey)?;
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
        nostr::event::Kind::from_u16(KIND_STORY),
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
    super::db::upsert_post_row(
        event_id,
        author_pubkey,
        content_str,
        KIND_STORY as i64,
        soshal_common_core::format::now_secs(),
        serde_json::to_string(&vec![
            vec!["d".to_string(), "soshal_story".to_string()],
            vec!["expiration".to_string(), expiry.to_string()],
        ])
        .unwrap_or_default(),
        None,
    )?;
    Ok(signed_json).into()
}

/// Fetch a user's stories (unexpired only).
#[frb(sync, serialize)]
pub fn streaming_fetch_stories(user_pubkey: String) -> Result<String, String> {
    let now = soshal_common_core::format::now_secs();
    let rows = super::db::db_query_json(
        &format!(
            "SELECT id, pubkey, content, created_at, tags_json, 0 AS views FROM posts \
             WHERE kind = {KIND_STORY} AND pubkey = ?1 AND is_deleted = 0 \
             ORDER BY created_at DESC LIMIT 50"
        ),
        &[user_pubkey],
    )?;
    super::util::json_ok(
        rows.into_iter()
            .filter_map(|v| story_from_value(&v))
            .filter(|s| s.expires_at == 0 || s.expires_at > now as u64)
            .collect::<Vec<_>>(),
    )
}

/// Fetch stories across accounts filtered by audience (public = all,
/// friends/network = reachable author set).
#[frb(sync, serialize)]
pub fn streaming_fetch_followed_stories(audience: String) -> Result<String, String> {
    let now = soshal_common_core::format::now_secs();
    let authors = super::identity::resolve_audience_authors(&audience)?;
    if let Some(a) = &authors {
        if a.is_empty() {
            return super::util::json_ok(Vec::<serde_json::Value>::new());
        }
    }
    let mut params: Vec<String> = Vec::new();
    let author_clause = match &authors {
        Some(a) => {
            params.push(serde_json::to_string(a).map_err(|e| format!("authors: {e}"))?);
            " AND p.pubkey IN (SELECT value FROM json_each(?1))"
        }
        None => "",
    };
    let rows = super::db::db_query_json(
        &format!(
            "SELECT p.id, p.pubkey, p.content, p.created_at, p.tags_json, 0 AS views FROM posts p \
             WHERE p.kind = {KIND_STORY} AND p.is_deleted = 0{author_clause} \
             ORDER BY p.created_at DESC LIMIT 200"
        ),
        &params,
    )?;
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

/// Check live stream availability and report transport status, echoing the
/// requesting subscriber pubkey. network-core keeps no per-subscriber
/// registry; the actual subscription happens per-peer via
/// `p2p_moq_subscribe_fetch` (QUIC stream fetch) on the subscriber side and
/// `serve_moq_subscription` on the publisher side.
#[frb(sync, serialize)]
pub fn streaming_moq_subscribe_stream(
    stream_id: String,
    subscriber_pubkey: String,
) -> Result<String, String> {
    let known = soshal_network_core::quic::moq_stream_known(&stream_id);
    super::util::json_ok(serde_json::json!({
        "status": if known { "live" } else { "unknown" },
        "stream_id": stream_id,
        "subscriber": subscriber_pubkey,
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
    fn test_mark_status_ended_hostile_one_element_tag() {
        let mut tags: Vec<Vec<String>> = vec![vec!["status".to_string()]];
        mark_status_ended(&mut tags);
        assert_eq!(
            tags,
            vec![vec!["status".to_string()]],
            "len-1 tag untouched"
        );

        let mut tags: Vec<Vec<String>> = vec![vec!["status".to_string(), "live".to_string()]];
        mark_status_ended(&mut tags);
        assert_eq!(tags, vec![vec!["status".to_string(), "ended".to_string()]]);
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
