//! Legacy RN domain FFI module
//!
//! Dead-legacy tables ported to Turso in migration v018 — full storage
//! access for future wiring. Nine domains: huddle posts, guestbook
//! entries, live stream chat, link previews, friend backups, geohash
//! peers (spatial peer discovery), custom profile nodes, do-not-refetch
//! markers, and diagnostic logs. All `legacy_*` fns are synchronous
//! DB-backed operations.

use flutter_rust_bridge::frb;

/// Store a huddle post (expiring ephemeral room message).
#[frb(sync, serialize)]
pub fn legacy_huddle_post_store(
    huddle_id: String,
    pubkey: String,
    content: String,
    expires_in_secs: i64,
) -> Result<bool, String> {
    let content = soshal_common_core::format::truncate(&content, 4000);
    let now = soshal_common_core::format::now_secs();
    let ttl = if expires_in_secs <= 0 {
        3600
    } else {
        expires_in_secs.min(7 * 24 * 3600)
    };
    let row = soshal_db_core::repos::huddle_post::HuddlePostRow {
        id: format!("huddle_{now}_{:x}", rand::random::<u32>()),
        huddle_id,
        pubkey,
        content,
        created_at: now,
        expires_at: now + ttl,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::huddle_post::HuddlePostRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// Fetch huddle posts (json array, oldest first; unexpired only).
#[frb(sync, serialize)]
pub fn legacy_huddle_posts(huddle_id: String, limit: i32) -> Result<String, String> {
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::huddle_post::HuddlePostRepo::new(db).list_by_huddle(
            &huddle_id,
            limit as i64,
            false,
        )
    })?;
    super::util::json_ok(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "huddle_id": r.huddle_id,
                    "pubkey": r.pubkey,
                    "content": r.content,
                    "created_at": r.created_at,
                    "expires_at": r.expires_at,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Delete a huddle post by id.
#[frb(sync, serialize)]
pub fn legacy_huddle_post_delete(post_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::huddle_post::HuddlePostRepo::new(db).delete(&post_id)?;
        Ok(true)
    })
}

/// Add a guestbook entry on a profile.
#[frb(sync, serialize)]
pub fn legacy_guestbook_add(
    profile_pubkey: String,
    sender_pubkey: String,
    sender_name: Option<String>,
    content: String,
) -> Result<bool, String> {
    let content = soshal_common_core::format::truncate(&content, 2000);
    if content.is_empty() {
        return Err("content must not be empty".to_string()).into();
    }
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::guestbook::GuestbookEntryRow {
        id: format!("guestbook_{now}_{:x}", rand::random::<u32>()),
        profile_pubkey,
        sender_pubkey,
        sender_name: sender_name.filter(|s| !s.is_empty()),
        sender_avatar: None,
        content,
        created_at: now,
        signature: None,
        approved: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// Fetch guestbook entries for a profile (json array, newest first).
#[frb(sync, serialize)]
pub fn legacy_guestbook_entries(
    profile_pubkey: String,
    limit: i32,
    only_approved: bool,
) -> Result<String, String> {
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db).list_by_profile(
            &profile_pubkey,
            limit as i64,
            only_approved,
        )
    })?;
    super::util::json_ok(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "profile_pubkey": r.profile_pubkey,
                    "sender_pubkey": r.sender_pubkey,
                    "sender_name": r.sender_name,
                    "sender_avatar": r.sender_avatar,
                    "content": r.content,
                    "created_at": r.created_at,
                    "approved": r.approved,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Approve or hide a guestbook entry.
#[frb(sync, serialize)]
pub fn legacy_guestbook_set_approved(entry_id: String, approved: bool) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db)
            .set_approved(&entry_id, approved)?;
        Ok(true)
    })
}

/// Delete a guestbook entry.
#[frb(sync, serialize)]
pub fn legacy_guestbook_delete(entry_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db).delete(&entry_id)?;
        Ok(true)
    })
}

/// Send a live-stream chat message.
#[frb(sync, serialize)]
pub fn legacy_stream_chat_send(
    stream_id: String,
    pubkey: String,
    text: String,
) -> Result<bool, String> {
    let text = soshal_common_core::format::truncate(&text, 1000);
    if text.is_empty() {
        return Err("text must not be empty".to_string()).into();
    }
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::stream_chat::StreamChatRow {
        id: format!("streamchat_{now}_{:x}", rand::random::<u32>()),
        stream_id,
        pubkey,
        text,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::stream_chat::StreamChatRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// Fetch live-stream chat messages (json array, oldest first).
#[frb(sync, serialize)]
pub fn legacy_stream_chat_messages(stream_id: String, limit: i32) -> Result<String, String> {
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::stream_chat::StreamChatRepo::new(db)
            .list_by_stream(&stream_id, limit as i64)
    })?;
    super::util::json_ok(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "stream_id": r.stream_id,
                    "pubkey": r.pubkey,
                    "text": r.text,
                    "created_at": r.created_at,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Clear chat for a stream.
#[frb(sync, serialize)]
pub fn legacy_stream_chat_clear(stream_id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::stream_chat::StreamChatRepo::new(db)
            .delete_for_stream(&stream_id)?;
        Ok(true)
    })
}

/// Store a link preview cache entry.
#[frb(sync, serialize)]
pub fn legacy_link_preview_store(
    url: String,
    title: String,
    description: String,
) -> Result<bool, String> {
    let row = soshal_db_core::repos::link_preview::LinkPreviewRow {
        url: url.clone(),
        domain: soshal_common_core::url::domain(&url).unwrap_or_default(),
        title: soshal_common_core::format::truncate(&title, 500),
        description: soshal_common_core::format::truncate(&description, 2000),
        image: None,
        favicon: None,
        cached_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::link_preview::LinkPreviewRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Fetch a cached link preview (null when absent).
#[frb(sync, serialize)]
pub fn legacy_link_preview_get(url: String) -> Result<Option<String>, String> {
    let row = super::db::with_db_result(|db| {
        soshal_db_core::repos::link_preview::LinkPreviewRepo::new(db).get(&url)
    })?;
    Ok(row.map(|r| {
        serde_json::to_string(&serde_json::json!({
            "url": r.url,
            "domain": r.domain,
            "title": r.title,
            "description": r.description,
            "image": r.image,
            "favicon": r.favicon,
            "cached_at": r.cached_at,
        }))
        .unwrap_or_default()
    }))
}

/// Store an encrypted friend-list backup.
#[frb(sync, serialize)]
pub fn legacy_friend_backup_store(
    user_pubkey: String,
    encrypted_data: String,
) -> Result<bool, String> {
    let row = soshal_db_core::repos::friend_backup::FriendBackupRow {
        user_pubkey,
        encrypted_data,
        updated_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::friend_backup::FriendBackupRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Fetch a friend backup (null when absent).
#[frb(sync, serialize)]
pub fn legacy_friend_backup_get(user_pubkey: String) -> Result<Option<String>, String> {
    let row = super::db::with_db_result(|db| {
        soshal_db_core::repos::friend_backup::FriendBackupRepo::new(db).get(&user_pubkey)
    })?;
    Ok(row.map(|r| r.encrypted_data))
}

/// Delete a friend backup.
#[frb(sync, serialize)]
pub fn legacy_friend_backup_delete(user_pubkey: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::friend_backup::FriendBackupRepo::new(db).delete(&user_pubkey)?;
        Ok(true)
    })
}

/// Upsert a geohash peer (spatial peer discovery).
#[frb(sync, serialize)]
pub fn legacy_geohash_peer_upsert(
    pubkey: String,
    geohash: String,
    purpose: String,
) -> Result<bool, String> {
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::geohash_peer::GeohashPeerRow {
        pubkey,
        geohash,
        purpose: if purpose.is_empty() {
            "both".to_string()
        } else {
            purpose
        },
        first_seen: now,
        last_seen: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::geohash_peer::GeohashPeerRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// List peers by geohash cell (json array).
#[frb(sync, serialize)]
pub fn legacy_geohash_peers_by_cell(geohash: String) -> Result<String, String> {
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::geohash_peer::GeohashPeerRepo::new(db).list_by_geohash(&geohash)
    })?;
    super::util::json_ok(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "pubkey": r.pubkey,
                    "geohash": r.geohash,
                    "purpose": r.purpose,
                    "first_seen": r.first_seen,
                    "last_seen": r.last_seen,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Purge stale geohash peers (unseen for `stale_secs`).
#[frb(sync, serialize)]
pub fn legacy_geohash_peers_purge(stale_secs: i64) -> Result<u64, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::geohash_peer::GeohashPeerRepo::new(db)
            .purge_stale(stale_secs.max(60))
    })
}

/// Upsert a custom profile node (layout slot). `layout` = JSON
/// `{"row":0,"col":0,"sort":0}`.
#[frb(sync, serialize)]
pub fn legacy_profile_node_upsert(
    id: String,
    user_pubkey: String,
    node_type: String,
    styles: String,
    properties: String,
    layout: String,
) -> Result<bool, String> {
    let layout: serde_json::Value =
        serde_json::from_str(&layout).unwrap_or_else(|_| serde_json::json!({}));
    let row = soshal_db_core::repos::profile_node::ProfileNodeRow {
        id,
        user_pubkey,
        node_type,
        styles,
        properties,
        layout_row: layout["row"].as_i64().unwrap_or(0),
        layout_col: layout["col"].as_i64().unwrap_or(0),
        sort_order: layout["sort"].as_i64().unwrap_or(0),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::profile_node::ProfileNodeRepo::new(db).upsert(&row)?;
        Ok(true)
    })
}

/// Fetch custom profile nodes (json array, sort order asc).
#[frb(sync, serialize)]
pub fn legacy_profile_nodes(user_pubkey: String) -> Result<String, String> {
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::profile_node::ProfileNodeRepo::new(db).list_by_user(&user_pubkey)
    })?;
    super::util::json_ok(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "user_pubkey": r.user_pubkey,
                    "type": r.node_type,
                    "styles": r.styles,
                    "properties": r.properties,
                    "layout_row": r.layout_row,
                    "layout_col": r.layout_col,
                    "sort_order": r.sort_order,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Delete a custom profile node (or all for a user when id is empty).
#[frb(sync, serialize)]
pub fn legacy_profile_node_delete(id: String, user_pubkey: String) -> Result<u64, String> {
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::profile_node::ProfileNodeRepo::new(db);
        if id.is_empty() {
            repo.delete_all_for(&user_pubkey)
        } else {
            repo.delete(&id)?;
            Ok(1)
        }
    })
}

/// Mark an item do-not-refetch (e.g. after repeated fetch failures).
#[frb(sync, serialize)]
pub fn legacy_refetch_block(id: String, reason: String) -> Result<bool, String> {
    let row = soshal_db_core::repos::refetch_item::RefetchItemRow {
        id,
        pubkey: None,
        reason: if reason.is_empty() {
            None
        } else {
            Some(soshal_common_core::format::truncate(&reason, 500))
        },
        created_at: soshal_common_core::format::now_secs(),
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::refetch_item::RefetchItemRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// Whether an item is marked do-not-refetch.
#[frb(sync, serialize)]
pub fn legacy_refetch_blocked(id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::refetch_item::RefetchItemRepo::new(db).contains(&id)
    })
}

/// Unblock an item for refetching.
#[frb(sync, serialize)]
pub fn legacy_refetch_unblock(id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::refetch_item::RefetchItemRepo::new(db).delete(&id)?;
        Ok(true)
    })
}

/// Append a diagnostic log entry (device-side capture).
#[frb(sync, serialize)]
pub fn legacy_diagnostic_log(
    level: String,
    service: String,
    method: String,
    message: String,
) -> Result<bool, String> {
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::diagnostic_log::DiagnosticLogRow {
        id: format!("diag_{now}_{:x}", rand::random::<u32>()),
        level: soshal_common_core::format::truncate(&level, 16),
        service: soshal_common_core::format::truncate(&service, 64),
        method: soshal_common_core::format::truncate(&method, 128),
        message: soshal_common_core::format::truncate(&message, 2000),
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::diagnostic_log::DiagnosticLogRepo::new(db).insert(&row)?;
        Ok(true)
    })
}

/// Fetch diagnostic logs (json array, newest first; optional level filter).
#[frb(sync, serialize)]
pub fn legacy_diagnostic_logs(limit: i32, level: Option<String>) -> Result<String, String> {
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::diagnostic_log::DiagnosticLogRepo::new(db)
            .list(limit as i64, level.as_deref())
    })?;
    super::util::json_ok(
        rows.iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "level": r.level,
                    "service": r.service,
                    "method": r.method,
                    "message": r.message,
                    "created_at": r.created_at,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Purge diagnostic logs older than `older_than_secs`.
#[frb(sync, serialize)]
pub fn legacy_diagnostic_purge(older_than_secs: i64) -> Result<u64, String> {
    super::db::with_db_result(|db| {
        soshal_db_core::repos::diagnostic_log::DiagnosticLogRepo::new(db)
            .purge_before(older_than_secs.max(60))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;
    use std::sync::Mutex;

    static DB_TEST_LOCK: Mutex<()> = Mutex::new(());
    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn tmp_db(label: &str) -> String {
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = format!(
            "{}/soshal_legacy_{label}_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            n
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        db::db_init(path.clone()).unwrap();
        path
    }

    fn insert_user(pubkey: &str, name: &str) {
        db::db_execute_raw(format!(
            "INSERT INTO users (pubkey, npub, name) VALUES ('{pubkey}','npub1{pubkey}','{name}') ON CONFLICT DO UPDATE SET name='{name}'"
        ))
        .unwrap();
    }

    fn parse_arr(json: &str) -> Vec<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn test_huddle_roundtrip_and_delete() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("huddle");
        assert!(parse_arr(&legacy_huddle_posts("h1".to_string(), 10).unwrap()).is_empty());
        assert!(legacy_huddle_post_store(
            "h1".to_string(),
            "pk1".to_string(),
            "first huddle note".to_string(),
            600
        )
        .unwrap());
        let arr = parse_arr(&legacy_huddle_posts("h1".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["huddle_id"], "h1");
        assert_eq!(arr[0]["pubkey"], "pk1");
        assert_eq!(arr[0]["content"], "first huddle note");
        let id = arr[0]["id"].as_str().unwrap().to_string();
        assert!(legacy_huddle_post_delete(id).unwrap());
        assert!(parse_arr(&legacy_huddle_posts("h1".to_string(), 10).unwrap()).is_empty());
    }

    #[test]
    fn test_huddle_ttl_clamps_low_and_high() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("ttl");
        legacy_huddle_post_store("h1".to_string(), "pk1".to_string(), "a".to_string(), 0).unwrap();
        let arr = parse_arr(&legacy_huddle_posts("h1".to_string(), 10).unwrap());
        assert_eq!(
            arr[0]["expires_at"].as_i64().unwrap() - arr[0]["created_at"].as_i64().unwrap(),
            3600
        );
        legacy_huddle_post_store(
            "h2".to_string(),
            "pk1".to_string(),
            "b".to_string(),
            999_999_999,
        )
        .unwrap();
        let arr = parse_arr(&legacy_huddle_posts("h2".to_string(), 10).unwrap());
        assert_eq!(
            arr[0]["expires_at"].as_i64().unwrap() - arr[0]["created_at"].as_i64().unwrap(),
            7 * 24 * 3600
        );
    }

    #[test]
    fn test_guestbook_add_entries_approve_filter() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("guestbook");
        assert!(legacy_guestbook_add(
            "prof1".to_string(),
            "pk1".to_string(),
            Some("alice".to_string()),
            "hi there".to_string()
        )
        .unwrap());
        let arr = parse_arr(&legacy_guestbook_entries("prof1".to_string(), 10, false).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["sender_pubkey"], "pk1");
        assert_eq!(arr[0]["sender_name"], "alice");
        assert_eq!(arr[0]["content"], "hi there");
        assert_eq!(arr[0]["approved"], false);
        let id = arr[0]["id"].as_str().unwrap().to_string();
        assert!(
            parse_arr(&legacy_guestbook_entries("prof1".to_string(), 10, true).unwrap()).is_empty()
        );
        assert!(legacy_guestbook_set_approved(id.clone(), true).unwrap());
        let arr = parse_arr(&legacy_guestbook_entries("prof1".to_string(), 10, true).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["approved"], true);
        assert!(legacy_guestbook_delete(id).unwrap());
        assert!(
            parse_arr(&legacy_guestbook_entries("prof1".to_string(), 10, false).unwrap())
                .is_empty()
        );
    }

    #[test]
    fn test_guestbook_newest_first_and_empty_content_err() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("guestorder");
        assert!(legacy_guestbook_add(
            "prof1".to_string(),
            "pk1".to_string(),
            None,
            "old".to_string()
        )
        .unwrap());
        db::db_execute_raw("UPDATE guestbook_entries SET created_at=1".to_string()).unwrap();
        assert!(legacy_guestbook_add(
            "prof1".to_string(),
            "pk2".to_string(),
            None,
            "new".to_string()
        )
        .unwrap());
        let arr = parse_arr(&legacy_guestbook_entries("prof1".to_string(), 10, false).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["content"], "new");
        assert!(
            legacy_guestbook_add("prof1".to_string(), "pk1".to_string(), None, String::new())
                .is_err()
        );
        assert!(
            parse_arr(&legacy_guestbook_entries("nobody".to_string(), 10, false).unwrap())
                .is_empty()
        );
    }

    #[test]
    fn test_stream_chat_roundtrip_and_clear() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("stream");
        assert!(parse_arr(&legacy_stream_chat_messages("s1".to_string(), 10).unwrap()).is_empty());
        assert!(
            legacy_stream_chat_send("s1".to_string(), "pk1".to_string(), "first".to_string())
                .unwrap()
        );
        db::db_execute_raw("UPDATE stream_chat SET created_at=1".to_string()).unwrap();
        assert!(
            legacy_stream_chat_send("s1".to_string(), "pk2".to_string(), "second".to_string())
                .unwrap()
        );
        assert!(
            legacy_stream_chat_send("s1".to_string(), "pk3".to_string(), String::new()).is_err()
        );
        let arr = parse_arr(&legacy_stream_chat_messages("s1".to_string(), 10).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["text"], "first");
        assert_eq!(arr[1]["pubkey"], "pk2");
        assert!(legacy_stream_chat_clear("s1".to_string()).unwrap());
        assert!(parse_arr(&legacy_stream_chat_messages("s1".to_string(), 10).unwrap()).is_empty());
    }

    #[test]
    fn test_link_preview_store_get_upsert_and_missing() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("preview");
        assert!(legacy_link_preview_get("https://example.com/a".to_string())
            .unwrap()
            .is_none());
        assert!(legacy_link_preview_store(
            "https://example.com/a".to_string(),
            "first title".to_string(),
            "first desc".to_string()
        )
        .unwrap());
        let json = legacy_link_preview_get("https://example.com/a".to_string())
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["url"], "https://example.com/a");
        assert_eq!(v["domain"], "example.com");
        assert_eq!(v["title"], "first title");
        legacy_link_preview_store(
            "https://example.com/a".to_string(),
            "second title".to_string(),
            "second desc".to_string(),
        )
        .unwrap();
        let json = legacy_link_preview_get("https://example.com/a".to_string())
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["title"], "second title");
    }

    #[test]
    fn test_friend_backup_store_get_delete() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("friendbackup");
        assert!(legacy_friend_backup_get("pk1".to_string())
            .unwrap()
            .is_none());
        assert!(legacy_friend_backup_store("pk1".to_string(), "enc-v1".to_string()).unwrap());
        assert_eq!(
            legacy_friend_backup_get("pk1".to_string())
                .unwrap()
                .unwrap(),
            "enc-v1"
        );
        legacy_friend_backup_store("pk1".to_string(), "enc-v2".to_string()).unwrap();
        assert_eq!(
            legacy_friend_backup_get("pk1".to_string())
                .unwrap()
                .unwrap(),
            "enc-v2"
        );
        assert!(legacy_friend_backup_delete("pk1".to_string()).unwrap());
        assert!(legacy_friend_backup_get("pk1".to_string())
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_geohash_upsert_list_and_purge() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("geohash");
        assert!(
            legacy_geohash_peer_upsert("pk1".to_string(), "u33d".to_string(), String::new())
                .unwrap()
        );
        assert!(legacy_geohash_peer_upsert(
            "pk2".to_string(),
            "u33d".to_string(),
            "find".to_string()
        )
        .unwrap());
        assert!(legacy_geohash_peer_upsert(
            "pk3".to_string(),
            "u9x8".to_string(),
            "both".to_string()
        )
        .unwrap());
        let arr = parse_arr(&legacy_geohash_peers_by_cell("u33d".to_string()).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["purpose"], "both");
        assert!(legacy_geohash_peers_by_cell("zzzz".to_string())
            .unwrap()
            .contains("[]"));
        assert_eq!(legacy_geohash_peers_purge(3600).unwrap(), 0);
        db::db_execute_raw("UPDATE geohash_peers SET last_seen=1".to_string()).unwrap();
        assert_eq!(legacy_geohash_peers_purge(60).unwrap(), 3);
        assert!(parse_arr(&legacy_geohash_peers_by_cell("u33d".to_string()).unwrap()).is_empty());
    }

    #[test]
    fn test_profile_node_upsert_list_sort_and_delete() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("nodes");
        insert_user("pk1", "tester");
        assert!(legacy_profile_node_upsert(
            "n1".to_string(),
            "pk1".to_string(),
            "text".to_string(),
            "{}".to_string(),
            "{}".to_string(),
            r#"{"row":1,"col":2,"sort":2}"#.to_string()
        )
        .unwrap());
        assert!(legacy_profile_node_upsert(
            "n2".to_string(),
            "pk1".to_string(),
            "image".to_string(),
            "{}".to_string(),
            "{}".to_string(),
            r#"{"row":0,"col":0,"sort":1}"#.to_string()
        )
        .unwrap());
        let arr = parse_arr(&legacy_profile_nodes("pk1".to_string()).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["id"], "n2");
        assert_eq!(arr[0]["type"], "image");
        assert_eq!(arr[0]["layout_row"], 0);
        assert_eq!(arr[0]["layout_col"], 0);
        assert_eq!(arr[1]["layout_row"], 1);
        assert_eq!(arr[1]["layout_col"], 2);
        assert_eq!(arr[1]["sort_order"], 2);
        assert_eq!(
            legacy_profile_node_delete("n1".to_string(), "pk1".to_string()).unwrap(),
            1
        );
        assert_eq!(
            legacy_profile_node_delete(String::new(), "pk1".to_string()).unwrap(),
            1
        );
        assert!(parse_arr(&legacy_profile_nodes("pk1".to_string()).unwrap()).is_empty());
    }

    #[test]
    fn test_profile_node_bad_layout_defaults_zeros() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("badlayout");
        insert_user("pk1", "tester");
        legacy_profile_node_upsert(
            "n1".to_string(),
            "pk1".to_string(),
            "text".to_string(),
            "{}".to_string(),
            "{}".to_string(),
            "not json".to_string(),
        )
        .unwrap();
        let arr = parse_arr(&legacy_profile_nodes("pk1".to_string()).unwrap());
        assert_eq!(arr[0]["layout_row"], 0);
        assert_eq!(arr[0]["layout_col"], 0);
        assert_eq!(arr[0]["sort_order"], 0);
    }

    #[test]
    fn test_refetch_block_blocked_unblock() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("refetch");
        assert!(!legacy_refetch_blocked("item1".to_string()).unwrap());
        assert!(legacy_refetch_block("item1".to_string(), String::new()).unwrap());
        assert!(legacy_refetch_blocked("item1".to_string()).unwrap());
        assert!(legacy_refetch_unblock("item1".to_string()).unwrap());
        assert!(!legacy_refetch_blocked("item1".to_string()).unwrap());
    }

    #[test]
    fn test_diagnostic_log_list_filter_and_purge() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("diag");
        assert!(legacy_diagnostic_log(
            "info".to_string(),
            "feed".to_string(),
            "load".to_string(),
            "started".to_string()
        )
        .unwrap());
        db::db_execute_raw("UPDATE diagnostic_logs SET created_at=1".to_string()).unwrap();
        assert!(legacy_diagnostic_log(
            "error".to_string(),
            "sync".to_string(),
            "push".to_string(),
            "failed".to_string()
        )
        .unwrap());
        let arr = parse_arr(&legacy_diagnostic_logs(10, None).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["level"], "error");
        assert_eq!(arr[0]["service"], "sync");
        assert_eq!(arr[0]["method"], "push");
        assert_eq!(arr[0]["message"], "failed");
        let errs = parse_arr(&legacy_diagnostic_logs(10, Some("error".to_string())).unwrap());
        assert_eq!(errs.len(), 1, "json: {errs:?}");
        assert_eq!(legacy_diagnostic_purge(60).unwrap(), 1);
        let arr = parse_arr(&legacy_diagnostic_logs(10, None).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["level"], "error");
    }
}
