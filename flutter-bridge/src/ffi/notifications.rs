//! Notifications FFI module
//!
//! Notifications are DB-backed rows produced by the relay sync loop (mention,
//! like, reply, message, follow). Fetching, filtering, read-state, and
//! push-token registration all happen against local storage; no platform
//! notification SDK code lives in Dart.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};

/// Notification item
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NotificationItem {
    pub id: String,
    pub notification_type: String,
    pub from_pubkey: String,
    pub from_name: String,
    pub from_avatar: String,
    pub content_preview: String,
    pub event_id: Option<String>,
    pub created_at: u64,
    pub read: bool,
    pub action_url: String,
}

fn row_to_item(
    row: &NotificationRow,
    users: &std::collections::HashMap<String, (String, String)>,
) -> NotificationItem {
    let (name, avatar) = row
        .from_pubkey
        .as_deref()
        .and_then(|pk| users.get(pk))
        .cloned()
        .unwrap_or_default();
    NotificationItem {
        id: row.id.clone(),
        notification_type: row.type_.clone(),
        from_pubkey: row.from_pubkey.clone().unwrap_or_default(),
        from_name: name,
        from_avatar: avatar,
        content_preview: row.content.clone().unwrap_or_default(),
        event_id: row.event_id.clone(),
        created_at: row.created_at.max(0) as u64,
        read: row.is_read,
        action_url: String::new(),
    }
}

fn user_names() -> std::collections::HashMap<String, (String, String)> {
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let rows = soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, name, picture FROM users WHERE picture IS NOT NULL OR name IS NOT NULL",
            (),
            |r| {
                let pk: String = r.get(0)?;
                let name: Option<String> = r.get(1)?;
                let pic: Option<String> = r.get(2)?;
                Ok((pk, name.unwrap_or_default(), pic.unwrap_or_default()))
            },
        )?;
        let mut map = std::collections::HashMap::new();
        for (pk, name, pic) in rows {
            map.insert(pk, (name, pic));
        }
        Ok(map)
    })
    .unwrap_or_default()
}

fn require_db_rows(
    pubkey: &str,
    limit: i64,
    type_filter: Option<&str>,
) -> Result<Vec<NotificationItem>, String> {
    super::db::with_db_result(|db| {
        let repo = NotificationRepo::new(db);
        let users = user_names();
        let unread = repo.get_unread(pubkey, 1000)?;
        let rows: Vec<NotificationRow> = match type_filter {
            Some(t) => unread
                .into_iter()
                .filter(|r| r.type_ == t)
                .take(limit as usize)
                .collect(),
            None => unread.into_iter().take(limit as usize).collect(),
        };
        Ok(rows.iter().map(|r| row_to_item(r, &users)).collect())
    })
}

/// Fetch unread notifications.
#[frb(sync, serialize)]
pub fn notifications_fetch_unread(user_pubkey: String, limit: i32) -> Result<String, String> {
    super::util::json_ok(require_db_rows(
        &user_pubkey,
        limit.clamp(1, 100) as i64,
        None,
    )?)
}

/// Fetch all notifications with pagination (raw query over the last
/// `limit` rows).
#[frb(sync, serialize)]
pub fn notifications_fetch(user_pubkey: String, limit: i32, offset: i32) -> Result<String, String> {
    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    super::db::db_query_raw(format!(
        "SELECT * FROM notifications WHERE pubkey = '{}' ORDER BY created_at DESC LIMIT {} OFFSET {}",
        user_pubkey.replace('\'', "''"),
        limit,
        offset
    ))
}

/// Mark notification as read.
#[frb(sync, serialize)]
pub fn notifications_mark_read(notification_id: String) -> Result<bool, String> {
    super::db::db_execute_raw(format!(
        "UPDATE notifications SET is_read = 1 WHERE id = '{}' AND is_read = 0",
        notification_id.replace('\'', "''")
    ))
    .map(|affected| affected > 0)
    .into()
}

/// Mark all notifications as read for a user.
#[frb(sync, serialize)]
pub fn notifications_mark_all_read(user_pubkey: String) -> Result<bool, String> {
    drop(user_pubkey);
    super::db::db_execute_raw("UPDATE notifications SET is_read = 1".to_string())
        .map(|_| true)
        .into()
}

/// Delete a notification.
#[frb(sync, serialize)]
pub fn notifications_delete(notification_id: String) -> Result<bool, String> {
    super::db::db_execute_raw(format!(
        "DELETE FROM notifications WHERE id = '{}'",
        notification_id.replace('\'', "''")
    ))
    .map(|affected| affected > 0)
    .into()
}

/// Get unread count.
#[frb(sync, serialize)]
pub fn notifications_get_unread_count(user_pubkey: String) -> Result<i32, String> {
    let json = super::db::db_query_raw(format!(
        "SELECT COUNT(*) AS c FROM notifications WHERE pubkey = '{}' AND is_read = 0",
        user_pubkey.replace('\'', "''")
    ))?;
    let count = serde_json::from_str::<Vec<serde_json::Value>>(&json)
        .ok()
        .and_then(|rows| rows.first().and_then(|r| r["c"].as_i64()))
        .unwrap_or(0);
    Ok(count as i32).into()
}

/// Fetch notifications by type.
#[frb(sync, serialize)]
pub fn notifications_fetch_by_type(
    user_pubkey: String,
    notification_type: String,
    limit: i32,
) -> Result<String, String> {
    super::util::json_ok(require_db_rows(
        &user_pubkey,
        limit.clamp(1, 100) as i64,
        Some(&notification_type),
    )?)
}

/// Fetch mentions.
#[frb(sync, serialize)]
pub fn notifications_fetch_mentions(user_pubkey: String, limit: i32) -> Result<String, String> {
    notifications_fetch_by_type(user_pubkey, "mention".to_string(), limit)
}

/// Fetch likes/reactions.
#[frb(sync, serialize)]
pub fn notifications_fetch_reactions(user_pubkey: String, limit: i32) -> Result<String, String> {
    notifications_fetch_by_type(user_pubkey, "like".to_string(), limit)
}

/// Fetch replies.
#[frb(sync, serialize)]
pub fn notifications_fetch_replies(user_pubkey: String, limit: i32) -> Result<String, String> {
    notifications_fetch_by_type(user_pubkey, "reply".to_string(), limit)
}

/// Fetch messages (new DMs).
#[frb(sync, serialize)]
pub fn notifications_fetch_messages(user_pubkey: String, limit: i32) -> Result<String, String> {
    notifications_fetch_by_type(user_pubkey, "message".to_string(), limit)
}

/// Fetch follows.
#[frb(sync, serialize)]
pub fn notifications_fetch_follows(user_pubkey: String, limit: i32) -> Result<String, String> {
    notifications_fetch_by_type(user_pubkey, "follow".to_string(), limit)
}

/// Register the platform push token for the active account.
#[frb(sync, serialize)]
pub fn notifications_register_push(user_pubkey: String, token: String) -> Result<bool, String> {
    drop(user_pubkey);
    if token.is_empty() || token.len() > 4096 {
        return Err("invalid push token".to_string()).into();
    }
    super::session::session_register_push_token(token)
}

/// Unregister from push notifications.
#[frb(sync, serialize)]
pub fn notifications_unregister_push(user_pubkey: String) -> Result<bool, String> {
    drop(user_pubkey);
    super::session::session_register_push_token(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{db, session};
    use std::sync::Mutex;

    static DB_TEST_LOCK: Mutex<()> = Mutex::new(());
    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn tmp_db(label: &str) -> String {
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = format!(
            "{}/soshal_notif_{label}_{}_{}.db",
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

    fn insert_notification(
        id: &str,
        pubkey: &str,
        type_: &str,
        from_pubkey: Option<&str>,
        content: &str,
        created_at: i64,
        is_read: bool,
    ) {
        db::db_execute_raw(format!(
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES ('{pubkey}','npub1{pubkey}')"
        ))
        .unwrap();
        db::with_db_result(|db| {
            NotificationRepo::new(db).upsert(&NotificationRow {
                id: id.to_string(),
                pubkey: pubkey.to_string(),
                type_: type_.to_string(),
                event_id: Some(format!("ev_{id}")),
                from_pubkey: from_pubkey.map(|s| s.to_string()),
                content: Some(content.to_string()),
                created_at,
                is_read,
            })
        })
        .unwrap();
    }

    fn insert_user(pubkey: &str, name: &str) {
        db::db_execute_raw(format!(
            "INSERT INTO users (pubkey, npub, name) VALUES ('{pubkey}','npub1{pubkey}','{name}') ON CONFLICT DO UPDATE SET name='{name}'"
        ))
        .unwrap();
    }

    #[test]
    fn test_fetch_unread_happy() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("unread");
        insert_user("pk1", "tester");
        insert_notification("n1", "pk1", "mention", Some("pk1"), "hi", 3000, false);
        insert_notification("n2", "pk1", "like", Some("pk1"), "like", 2000, false);
        insert_notification("n3", "pk1", "follow", Some("pk1"), "read", 1000, true);
        insert_notification("n4", "pk2", "mention", Some("pk2"), "other", 1000, false);
        let json = notifications_fetch_unread("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["id"], "n1");
        assert_eq!(arr[0]["notification_type"], "mention");
        assert_eq!(arr[0]["from_pubkey"], "pk1");
        assert_eq!(arr[0]["from_name"], "tester");
        assert_eq!(arr[0]["read"], false);
        assert!(arr.iter().all(|n| n["pubkey"] != "pk2"));
    }

    #[test]
    fn test_fetch_unread_limit_clamped() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("clamp");
        for i in 0..3 {
            insert_notification(
                &format!("n{i}"),
                "pk1",
                "mention",
                None,
                "x",
                3000 - i,
                false,
            );
        }
        let json = notifications_fetch_unread("pk1".to_string(), 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        let json = notifications_fetch_unread("pk1".to_string(), -5).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn test_fetch_unread_empty() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("empty");
        insert_notification("n1", "pk1", "mention", None, "x", 1000, false);
        let json = notifications_fetch_unread("nobody".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_fetch_paginated_includes_read() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("fetch");
        insert_notification("n1", "pk1", "mention", None, "a", 3000, false);
        insert_notification("n2", "pk1", "like", None, "b", 2000, true);
        insert_notification("n3", "pk1", "follow", None, "c", 1000, false);
        let json = notifications_fetch("pk1".to_string(), 2, 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "n1");
        assert_eq!(arr[1]["id"], "n2");
        let json = notifications_fetch("pk1".to_string(), 10, 2).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "n3");
    }

    #[test]
    fn test_mark_read() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("markread");
        insert_notification("n1", "pk1", "mention", None, "x", 1000, false);
        assert!(notifications_mark_read("n1".to_string()).unwrap());
        assert_eq!(
            notifications_get_unread_count("pk1".to_string()).unwrap(),
            0
        );
        let json = notifications_fetch_unread("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert!(arr.is_empty());
        assert!(!notifications_mark_read("n1".to_string()).unwrap());
    }

    #[test]
    fn test_mark_all_read() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("markall");
        insert_notification("n1", "pk1", "mention", None, "x", 2000, false);
        insert_notification("n2", "pk1", "like", None, "x", 1000, false);
        insert_notification("n3", "pk2", "follow", None, "x", 1000, false);
        assert!(notifications_mark_all_read("pk1".to_string()).unwrap());
        assert_eq!(
            notifications_get_unread_count("pk1".to_string()).unwrap(),
            0
        );
        assert_eq!(
            notifications_get_unread_count("pk2".to_string()).unwrap(),
            0
        );
    }

    #[test]
    fn test_delete() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("delete");
        insert_notification("n1", "pk1", "mention", None, "x", 1000, false);
        assert!(notifications_delete("n1".to_string()).unwrap());
        assert!(!notifications_delete("n1".to_string()).unwrap());
        let json = notifications_fetch("pk1".to_string(), 10, 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_get_unread_count() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("count");
        insert_notification("n1", "pk1", "mention", None, "x", 3000, false);
        insert_notification("n2", "pk1", "like", None, "x", 2000, false);
        insert_notification("n3", "pk1", "follow", None, "x", 1000, true);
        assert_eq!(
            notifications_get_unread_count("pk1".to_string()).unwrap(),
            2
        );
        assert_eq!(
            notifications_get_unread_count("pk2".to_string()).unwrap(),
            0
        );
    }

    #[test]
    fn test_fetch_by_type_filters() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("bytype");
        insert_notification("n1", "pk1", "mention", None, "x", 3000, false);
        insert_notification("n2", "pk1", "like", None, "x", 2000, false);
        insert_notification("n3", "pk1", "follow", None, "x", 1000, false);
        let json = notifications_fetch_by_type("pk1".to_string(), "like".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "n2");
        assert_eq!(arr[0]["notification_type"], "like");
    }

    #[test]
    fn test_fetch_mentions() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("mentions");
        insert_notification("n1", "pk1", "mention", None, "x", 1000, false);
        let json = notifications_fetch_mentions("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["notification_type"], "mention");
    }

    #[test]
    fn test_fetch_reactions() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("reactions");
        insert_notification("n1", "pk1", "like", None, "x", 1000, false);
        let json = notifications_fetch_reactions("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["notification_type"], "like");
    }

    #[test]
    fn test_fetch_replies() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("replies");
        insert_notification("n1", "pk1", "reply", None, "x", 1000, false);
        let json = notifications_fetch_replies("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["notification_type"], "reply");
    }

    #[test]
    fn test_fetch_messages() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("messages");
        insert_notification("n1", "pk1", "message", None, "x", 1000, false);
        let json = notifications_fetch_messages("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["notification_type"], "message");
    }

    #[test]
    fn test_fetch_follows() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("follows");
        insert_notification("n1", "pk1", "follow", None, "x", 1000, false);
        let json = notifications_fetch_follows("pk1".to_string(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["notification_type"], "follow");
    }

    #[test]
    fn test_push_token_register_and_unregister() {
        let _g = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("soshal_notif_push_{}_{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("app.db").to_string_lossy().to_string();
        db::db_init(db_path.clone()).unwrap();
        let session = r#"{"active_pubkey":"pk1","accounts":[{"pubkey":"pk1","npub":"npub1pk1","last_used":1,"relay_list":[]}]}"#;
        session::session_save(db_path.clone(), session.to_string()).unwrap();
        assert!(notifications_register_push("pk1".to_string(), "tok123".to_string()).unwrap());
        let reloaded = session::session_load(db_path.clone()).unwrap();
        assert!(reloaded.contains("tok123"));
        assert!(notifications_unregister_push("pk1".to_string()).unwrap());
        let reloaded = session::session_load(db_path.clone()).unwrap();
        assert!(!reloaded.contains("tok123"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_push_register_rejects_invalid_token() {
        let err = notifications_register_push("pk1".to_string(), String::new()).unwrap_err();
        assert!(err.contains("invalid push token"));
        let long = "x".repeat(4097);
        let err = notifications_register_push("pk1".to_string(), long).unwrap_err();
        assert!(err.contains("invalid push token"));
    }

    #[test]
    fn test_errors_when_db_not_initialized() {
        if db::db_path().is_err() {
            assert!(notifications_fetch_unread("pk1".to_string(), 10)
                .unwrap_err()
                .contains("not initialized"));
        }
    }
}
