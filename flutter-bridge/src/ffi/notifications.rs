//! Notifications FFI module
//!
//! Notifications are DB-backed rows produced by the relay sync loop (mention,
//! like, reply, message, follow). Fetching, filtering, read-state, and
//! push-token registration all happen against local storage; no platform
//! notification SDK code lives in Dart.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use soshal_db_core::error::DbError;
use soshal_db_core::repos::ignored_notification::IgnoredNotificationRepo;
use soshal_db_core::repos::notification::{NotificationRepo, NotificationRow};
use soshal_db_core::Database;

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
    row: NotificationRow,
    users: &std::collections::HashMap<String, (String, String)>,
) -> NotificationItem {
    let from_pk_clean = row
        .from_pubkey
        .as_deref()
        .map(|pk| pk.trim().to_ascii_lowercase());
    let (name, avatar) = from_pk_clean
        .as_deref()
        .and_then(|pk| users.get(pk))
        .cloned()
        .unwrap_or_default();
    NotificationItem {
        id: row.id,
        notification_type: row.type_,
        from_pubkey: row.from_pubkey.unwrap_or_default(),
        from_name: name,
        from_avatar: avatar,
        content_preview: row.content.unwrap_or_default(),
        event_id: row.event_id,
        created_at: row.created_at.max(0) as u64,
        read: row.is_read,
        action_url: String::new(),
    }
}

fn user_names(
    db: &Database,
    pubkeys: &[&str],
) -> Result<std::collections::HashMap<String, (String, String)>, DbError> {
    if pubkeys.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let conn = db.conn()?;
    if pubkeys.len() == 1 {
        let rows = soshal_db_core::query::query(
            &conn,
            "SELECT pubkey, name, picture FROM users WHERE pubkey = ?1",
            libsql::params![pubkeys[0]],
            |r| {
                let pk: String = r.get(0)?;
                let name: Option<String> = r.get(1)?;
                let pic: Option<String> = r.get(2)?;
                Ok((pk, name.unwrap_or_default(), pic.unwrap_or_default()))
            },
        )?;
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for (pk, name, pic) in rows {
            map.insert(pk.trim().to_ascii_lowercase(), (name, pic));
        }
        return Ok(map);
    }
    use std::fmt::Write;
    let mut sql = String::with_capacity(60 + pubkeys.len() * 4);
    sql.push_str("SELECT pubkey, name, picture FROM users WHERE pubkey IN (");
    for i in 0..pubkeys.len() {
        if i > 0 {
            sql.push(',');
        }
        let _ = write!(sql, "?{}", i + 1);
    }
    sql.push(')');
    let params = libsql::params_from_iter(pubkeys.iter().copied());
    let rows = soshal_db_core::query::query(&conn, &sql, params, |r| {
        let pk: String = r.get(0)?;
        let name: Option<String> = r.get(1)?;
        let pic: Option<String> = r.get(2)?;
        Ok((pk, name.unwrap_or_default(), pic.unwrap_or_default()))
    })?;
    let mut map = std::collections::HashMap::with_capacity(rows.len());
    for (pk, name, pic) in rows {
        map.insert(pk.trim().to_ascii_lowercase(), (name, pic));
    }
    Ok(map)
}

fn require_db_rows(
    pubkey: &str,
    limit: i64,
    type_filter: Option<&str>,
) -> Result<Vec<NotificationItem>, String> {
    super::db::with_db_result(|db| {
        let repo = NotificationRepo::new(db);
        let rows = match type_filter {
            Some(t) => repo.get_unread_filtered(pubkey, t, limit)?,
            None => repo.get_unread(pubkey, limit)?,
        };
        let mut from_pks_set = std::collections::HashSet::with_capacity(rows.len());
        for r in &rows {
            if let Some(pk) = r.from_pubkey.as_deref() {
                from_pks_set.insert(pk);
            }
        }
        let from_pks: Vec<&str> = from_pks_set.into_iter().collect();
        let users = user_names(db, &from_pks)?;
        Ok(rows.into_iter().map(|r| row_to_item(r, &users)).collect())
    })
}

/// Fetch unread notifications.
#[frb(sync, serialize)]
pub fn notifications_fetch_unread(user_pubkey: String, limit: i32) -> Result<String, String> {
    let user_pubkey = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&user_pubkey)?;
    super::util::json_ok(require_db_rows(
        &user_pubkey,
        limit.clamp(1, 100) as i64,
        None,
    )?)
}

/// Fetch all notifications with pagination (typed query over the last
/// `limit` rows).
#[frb(sync, serialize)]
pub fn notifications_fetch(user_pubkey: String, limit: i32, offset: i32) -> Result<String, String> {
    let user_pubkey = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&user_pubkey)?;
    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let rows: Vec<NotificationRow> = soshal_db_core::query::query_capacity(
            &conn,
            "SELECT id, pubkey, type, event_id, from_pubkey, content, created_at, is_read \
             FROM notifications WHERE LOWER(pubkey) = ?1 \
             AND NOT EXISTS (SELECT 1 FROM ignored_notifications i WHERE LOWER(i.pubkey) = ?1 AND (i.kind = notifications.type OR i.kind = 'user' OR i.kind = 'thread' OR i.kind = 'all') AND ((LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND i.event_id = '') OR (LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, '')) AND i.from_pubkey = '') OR (LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, ''))))) \
             ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            libsql::params![user_pubkey.as_str(), limit as i64, offset as i64],
            limit as usize,
            |r| {
                Ok(NotificationRow {
                    id: r.get(0)?,
                    pubkey: r.get(1)?,
                    type_: r.get(2)?,
                    event_id: r.get(3)?,
                    from_pubkey: r.get(4)?,
                    content: r.get(5)?,
                    created_at: r.get(6)?,
                    is_read: r.get(7)?,
                })
            },
        )?;
        let mut from_pks_set = std::collections::HashSet::with_capacity(rows.len());
        for r in &rows {
            if let Some(pk) = r.from_pubkey.as_deref() {
                from_pks_set.insert(pk);
            }
        }
        let from_pks: Vec<&str> = from_pks_set.into_iter().collect();
        let users = user_names(db, &from_pks)?;
        Ok(rows
            .into_iter()
            .map(|r| row_to_item(r, &users))
            .collect::<Vec<_>>())
    })
    .map(super::util::json_ok)?
}

/// Mark notification as read.
#[frb(sync, serialize)]
pub fn notifications_mark_read(notification_id: String) -> Result<bool, String> {
    let caller = super::signer::signer_pubkey()?;
    let caller_norm = caller.trim().to_ascii_lowercase();
    super::db::db_execute_params(
        "UPDATE notifications SET is_read = 1 WHERE id = ?1 AND LOWER(pubkey) = ?2 AND is_read = 0",
        &[notification_id, caller_norm],
    )
    .map(|affected| affected > 0)
    .into()
}

/// Mark all notifications as read for a user.
#[frb(sync, serialize)]
pub fn notifications_mark_all_read(user_pubkey: String) -> Result<bool, String> {
    let user_pubkey = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&user_pubkey)?;
    super::db::db_execute_params(
        "UPDATE notifications SET is_read = 1 WHERE LOWER(pubkey) = ?1",
        &[user_pubkey],
    )
    .map(|_| true)
    .into()
}

/// Delete a notification.
#[frb(sync, serialize)]
pub fn notifications_delete(notification_id: String) -> Result<bool, String> {
    let caller = super::signer::signer_pubkey()?;
    let caller_norm = caller.trim().to_ascii_lowercase();
    super::db::db_execute_params(
        "DELETE FROM notifications WHERE id = ?1 AND LOWER(pubkey) = ?2",
        &[notification_id, caller_norm],
    )
    .map(|affected| affected > 0)
    .into()
}

/// Persist an "ignore user" decision so the user's notifications stay
/// suppressed across fetches and app restarts.
#[frb(sync, serialize)]
pub fn notifications_ignore_user(user_pubkey: String, from_pubkey: String) -> Result<bool, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    let f_pk = from_pubkey.trim().to_ascii_lowercase();
    if u_pk.is_empty() || f_pk.is_empty() {
        return Err("bad ignore-user args".to_string()).into();
    }
    if u_pk == f_pk {
        return Err("cannot ignore self".to_string()).into();
    }
    super::signer::require_identity(&u_pk)?;
    super::db::with_db_result(|db| {
        let at = soshal_common_core::format::now_secs();
        IgnoredNotificationRepo::new(db)
            .ignore_user(&u_pk, &f_pk, "user", at)
            .map(|()| true)
    })
    .into()
}

/// Persist a "turn off thread/like notifications" decision.
#[frb(sync, serialize)]
pub fn notifications_ignore_thread(user_pubkey: String, event_id: String) -> Result<bool, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    if u_pk.is_empty() || event_id.is_empty() {
        return Err("bad ignore-thread args".to_string()).into();
    }
    super::signer::require_identity(&u_pk)?;
    super::db::with_db_result(|db| {
        let at = soshal_common_core::format::now_secs();
        IgnoredNotificationRepo::new(db)
            .ignore_thread(&u_pk, &event_id, "thread", at)
            .map(|()| true)
    })
    .into()
}

/// Remove an "ignore user" decision.
#[frb(sync, serialize)]
pub fn notifications_unignore_user(
    user_pubkey: String,
    from_pubkey: String,
) -> Result<bool, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    let f_pk = from_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&u_pk)?;
    super::db::with_db_result(|db| {
        IgnoredNotificationRepo::new(db)
            .unignore_user(&u_pk, &f_pk, "user")
            .map(|()| true)
    })
    .into()
}

/// Remove a "turn off thread" decision.
#[frb(sync, serialize)]
pub fn notifications_unignore_thread(
    user_pubkey: String,
    event_id: String,
) -> Result<bool, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&u_pk)?;
    super::db::with_db_result(|db| {
        IgnoredNotificationRepo::new(db)
            .unignore_thread(&u_pk, &event_id, "thread")
            .map(|()| true)
    })
    .into()
}

/// List all ignore rows for the Ignored List dashboard.
#[frb(sync, serialize)]
pub fn notifications_list_ignored(user_pubkey: String) -> Result<String, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&u_pk)?;
    #[derive(Serialize)]
    struct IgnoredRow<'a> {
        kind: &'a str,
        from_pubkey: &'a str,
        event_id: &'a str,
        created_at: i64,
    }
    let rows = super::db::with_db_result(|db| IgnoredNotificationRepo::new(db).list(&u_pk))?;
    let items: Vec<IgnoredRow> = rows
        .iter()
        .map(|(kind, from_pubkey, event_id, created_at)| IgnoredRow {
            kind,
            from_pubkey,
            event_id,
            created_at: *created_at,
        })
        .collect();
    super::util::json_ok(items)
}

/// Whether a notification (user + optional event) is currently ignored.
#[frb(sync, serialize)]
pub fn notifications_is_ignored(
    user_pubkey: String,
    kind: String,
    from_pubkey: String,
    event_id: String,
) -> Result<bool, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    let f_pk = from_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&u_pk)?;
    super::db::with_db_result(|db| {
        IgnoredNotificationRepo::new(db).is_ignored(
            &u_pk,
            kind.as_str(),
            f_pk.as_str(),
            event_id.as_str(),
        )
    })
    .into()
}

/// Get unread count.
#[frb(sync, serialize)]
pub fn notifications_get_unread_count(user_pubkey: String) -> Result<i32, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&u_pk)?;
    super::db::with_db_result(|db| {
        let count = NotificationRepo::new(db).count_unread(&u_pk)?;
        Ok(count as i32)
    })
}

/// Fetch notifications by type.
#[frb(sync, serialize)]
pub fn notifications_fetch_by_type(
    user_pubkey: String,
    notification_type: String,
    limit: i32,
) -> Result<String, String> {
    let u_pk = user_pubkey.trim().to_ascii_lowercase();
    super::signer::require_identity(&u_pk)?;
    super::util::json_ok(require_db_rows(
        &u_pk,
        limit.clamp(1, 100) as i64,
        Some(&notification_type),
    )?)
}

fn ensure_active_account(user_pubkey: &str) -> Result<(), String> {
    let json = super::session::session_get_active()?;
    let active_pubkey = serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v["pubkey"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    if !active_pubkey
        .trim()
        .eq_ignore_ascii_case(user_pubkey.trim())
    {
        return Err("push token must be registered for the active account".to_string());
    }
    Ok(())
}

/// Register the platform push token for the active account. The caller's
/// pubkey must match the active account.
#[frb(sync, serialize)]
pub fn notifications_register_push(user_pubkey: String, token: String) -> Result<bool, String> {
    if token.is_empty() || token.len() > 4096 {
        return Err("invalid push token".to_string()).into();
    }
    ensure_active_account(&user_pubkey)?;
    super::session::session_register_push_token(token)
}

/// Unregister from push notifications. The caller's pubkey must match the
/// active account.
#[frb(sync, serialize)]
pub fn notifications_unregister_push(user_pubkey: String) -> Result<bool, String> {
    ensure_active_account(&user_pubkey)?;
    super::session::session_register_push_token(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{db, session};

    fn tmp_db(label: &str) -> String {
        db::tmp_db(label, "notif")
    }

    fn setup_test_context(
        label: &str,
    ) -> (
        String,
        std::sync::MutexGuard<'static, ()>,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let db_lock = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let signer_lock = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _ = tmp_db(label);
        let keys = soshal_nostr_core::keys::generate_keys();
        let pubkey = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        (pubkey, db_lock, signer_lock)
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
        db::db_execute_params(
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, 'npub1' || ?1)",
            &[pubkey.to_string()],
        )
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
        db::db_execute_params(
            "INSERT INTO users (pubkey, npub, name) VALUES (?1, 'npub1' || ?1, ?2) ON CONFLICT DO UPDATE SET name=?2",
            &[pubkey.to_string(), name.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn test_fetch_unread_happy() {
        let (pk1, _g, _s) = setup_test_context("unread");
        insert_user(&pk1, "tester");
        insert_notification("n1", &pk1, "mention", Some(&pk1), "hi", 3000, false);
        insert_notification("n2", &pk1, "like", Some(&pk1), "like", 2000, false);
        insert_notification("n3", &pk1, "follow", Some(&pk1), "read", 1000, true);
        insert_notification("n4", "pk2", "mention", Some("pk2"), "other", 1000, false);
        let json = notifications_fetch_unread(pk1.clone(), 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["id"], "n1");
        assert_eq!(arr[0]["notification_type"], "mention");
        assert_eq!(arr[0]["from_pubkey"], pk1);
        assert_eq!(arr[0]["from_name"], "tester");
        assert_eq!(arr[0]["read"], false);
        assert!(arr.iter().all(|n| n["pubkey"] != "pk2"));
    }

    #[test]
    fn test_fetch_unread_limit_clamped() {
        let (pk1, _g, _s) = setup_test_context("clamp");
        for i in 0..3 {
            insert_notification(
                &format!("n{i}"),
                &pk1,
                "mention",
                None,
                "x",
                3000 - i,
                false,
            );
        }
        let json = notifications_fetch_unread(pk1.clone(), 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
        let json = notifications_fetch_unread(pk1, -5).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn test_fetch_unread_empty() {
        let (nobody, _g, _s) = setup_test_context("empty");
        insert_notification("n1", "pk1", "mention", None, "x", 1000, false);
        let json = notifications_fetch_unread(nobody, 10).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_fetch_paginated_includes_read() {
        let (pk1, _g, _s) = setup_test_context("fetch");
        insert_notification("n1", &pk1, "mention", None, "a", 3000, false);
        insert_notification("n2", &pk1, "like", None, "b", 2000, true);
        insert_notification("n3", &pk1, "follow", None, "c", 1000, false);
        let json = notifications_fetch(pk1.clone(), 2, 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "n1");
        assert_eq!(arr[1]["id"], "n2");
        let json = notifications_fetch(pk1, 10, 2).unwrap();
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
        let (pk1, _g, _s) = setup_test_context("markread");
        insert_notification("n1", &pk1, "mention", None, "x", 1000, false);
        assert!(notifications_mark_read("n1".to_string()).unwrap());
        assert_eq!(notifications_get_unread_count(pk1.clone()).unwrap(), 0);
        let json = notifications_fetch_unread(pk1, 10).unwrap();
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
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("markall");
        let keys1 = soshal_nostr_core::keys::generate_keys();
        let pk1 = keys1.public_key().to_hex();
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let pk2 = keys2.public_key().to_hex();
        super::super::signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        insert_notification("n1", &pk1, "mention", None, "x", 2000, false);
        insert_notification("n2", &pk1, "like", None, "x", 1000, false);
        insert_notification("n3", &pk2, "follow", None, "x", 1000, false);
        assert!(notifications_mark_all_read(pk1.clone()).unwrap());
        assert_eq!(notifications_get_unread_count(pk1).unwrap(), 0);

        // While pk1 is active, attempting to read pk2's unread count fails identity check
        assert!(notifications_get_unread_count(pk2.clone()).is_err());

        // Unlocking as pk2 allows reading pk2's unread count
        super::super::signer::signer_unlock(keys2.secret_key().to_secret_hex()).unwrap();
        assert_eq!(notifications_get_unread_count(pk2).unwrap(), 1);
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_ignore_unignore_auth() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("ignore_auth");
        let keys1 = soshal_nostr_core::keys::generate_keys();
        let pk1 = keys1.public_key().to_hex();
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let _pk2 = keys2.public_key().to_hex();

        // Locked signer fails
        assert!(notifications_ignore_user(pk1.clone(), "spammer".to_string()).is_err());

        // Unlocked as pk2 cannot modify pk1's ignore list
        super::super::signer::signer_unlock(keys2.secret_key().to_secret_hex()).unwrap();
        let err = notifications_ignore_user(pk1.clone(), "spammer".to_string());
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("identity mismatch"));

        // Unlocked as pk1 succeeds
        super::super::signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        assert!(notifications_ignore_user(pk1.clone(), "spammer".to_string()).unwrap());
        assert!(notifications_ignore_thread(pk1.clone(), "evt_123".to_string()).unwrap());

        assert!(notifications_unignore_user(pk1.clone(), "spammer".to_string()).unwrap());
        assert!(notifications_unignore_thread(pk1, "evt_123".to_string()).unwrap());
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_delete() {
        let (pk1, _g, _s) = setup_test_context("delete");
        insert_notification("n1", &pk1, "mention", None, "x", 1000, false);
        assert!(notifications_delete("n1".to_string()).unwrap());
        assert!(!notifications_delete("n1".to_string()).unwrap());
        let json = notifications_fetch(pk1, 10, 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert!(arr.is_empty());
    }

    #[test]
    fn test_register_push_requires_active_account() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("push");
        let n = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "soshal_session_push_{}_{}",
            std::process::id(),
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("app.db").to_string_lossy().to_string();
        let _ = crate::ffi::db::db_init(db_path.clone());
        session::session_load(db_path).unwrap();
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        let pk_other = soshal_nostr_core::keys::generate_keys()
            .public_key()
            .to_hex();
        session::session_add_account(pk.clone(), "npub1pk1valid".to_string(), "[]".to_string())
            .unwrap();
        // Wrong account for the active session.
        assert!(notifications_register_push(pk_other, "tok".to_string()).is_err());
        // Valid token for the active account.
        assert!(notifications_register_push(pk.clone(), "tok123".to_string()).unwrap());
        // Empty / oversized tokens rejected.
        assert!(notifications_register_push(pk.clone(), String::new()).is_err());
        assert!(notifications_register_push(pk.clone(), "x".repeat(5000)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_get_unread_count() {
        let (pk1, _g, _s) = setup_test_context("count");
        insert_notification("n1", &pk1, "mention", None, "x", 3000, false);
        insert_notification("n2", &pk1, "like", None, "x", 2000, false);
        insert_notification("n3", &pk1, "follow", None, "x", 1000, true);
        assert_eq!(notifications_get_unread_count(pk1.clone()).unwrap(), 2);
        assert!(notifications_get_unread_count("pk2".to_string()).is_err());
    }

    #[test]
    fn test_fetch_by_type_filters() {
        let (pk1, _g, _s) = setup_test_context("bytype");
        insert_notification("n1", &pk1, "mention", None, "x", 3000, false);
        insert_notification("n2", &pk1, "like", None, "x", 2000, false);
        insert_notification("n3", &pk1, "follow", None, "x", 1000, false);
        let json = notifications_fetch_by_type(pk1, "like".to_string(), 10).unwrap();
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
        let (pk1, _g, _s) = setup_test_context("mentions");
        insert_notification("n1", &pk1, "mention", None, "x", 1000, false);
        let json = notifications_fetch_by_type(pk1, "mention".to_string(), 10).unwrap();
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
        let (pk1, _g, _s) = setup_test_context("reactions");
        insert_notification("n1", &pk1, "like", None, "x", 1000, false);
        let json = notifications_fetch_by_type(pk1, "like".to_string(), 10).unwrap();
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
        let (pk1, _g, _s) = setup_test_context("replies");
        insert_notification("n1", &pk1, "reply", None, "x", 1000, false);
        let json = notifications_fetch_by_type(pk1, "reply".to_string(), 10).unwrap();
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
        let (pk1, _g, _s) = setup_test_context("messages");
        insert_notification("n1", &pk1, "message", None, "x", 1000, false);
        let json = notifications_fetch_by_type(pk1, "message".to_string(), 10).unwrap();
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
        let (pk1, _g, _s) = setup_test_context("follows");
        insert_notification("n1", &pk1, "follow", None, "x", 1000, false);
        let json = notifications_fetch_by_type(pk1, "follow".to_string(), 10).unwrap();
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
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let dir = soshal_test_util::tmp_root("notif_push");
        let db_path = dir.join("app.db").to_string_lossy().to_string();
        db::db_init(db_path.clone()).unwrap();
        let session = format!(
            r#"{{"active_pubkey":"pk1","accounts":[{{"pubkey":"pk1","npub":"npub1pk1","last_used":{},"relay_list":[]}}]}}"#,
            soshal_common_core::format::now_secs()
        );
        session::session_save(db_path.clone(), session.to_string()).unwrap();
        assert!(notifications_register_push("pk1".to_string(), "tok123".to_string()).unwrap());
        let reloaded = session::session_load(db_path.clone()).unwrap();
        assert!(reloaded.contains("tok123"));
        assert!(notifications_unregister_push("pk1".to_string()).unwrap());
        let reloaded = session::session_load(db_path).unwrap();
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
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        crate::ffi::db::reset_db_global();
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(notifications_fetch_unread(pk, 10)
            .unwrap_err()
            .contains("database not initialized"));
    }

    #[test]
    fn test_fetch_offset_and_limit_clamped() {
        let (pk1, _g, _s) = setup_test_context("offclamp");
        for i in 0..501 {
            insert_notification(
                &format!("n{i}"),
                &pk1,
                "mention",
                None,
                "x",
                4000 - i,
                false,
            );
        }
        // limit 600 clamps to 500; offset -1 clamps to 0.
        let json = notifications_fetch(pk1.clone(), 600, -1).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 500, "json: {json}");
        assert_eq!(arr[0]["id"], "n0");
        assert_eq!(arr[499]["id"], "n499");
        // limit 0 clamps to 1.
        let json = notifications_fetch(pk1.clone(), 0, 0).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1, "json: {json}");
        assert_eq!(arr[0]["id"], "n0");
        // Negative offset reads from the start.
        let json = notifications_fetch(pk1, 10, -1).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 10, "json: {json}");
    }

    #[test]
    fn test_fetch_by_type_limit_clamped() {
        let (pk1, _g, _s) = setup_test_context("byclamp");
        insert_notification("n1", &pk1, "like", None, "x", 3000, false);
        insert_notification("n2", &pk1, "like", None, "x", 2000, false);
        insert_notification("n3", &pk1, "like", None, "x", 1000, false);
        insert_notification("n4", &pk1, "mention", None, "x", 500, false);
        for limit in [0, -5] {
            let json = notifications_fetch_by_type(pk1.clone(), "like".to_string(), limit).unwrap();
            let arr = serde_json::from_str::<serde_json::Value>(&json)
                .unwrap()
                .as_array()
                .unwrap()
                .clone();
            assert_eq!(arr.len(), 1, "limit {limit}: {json}");
            assert_eq!(arr[0]["notification_type"], "like");
        }
        let json = notifications_fetch_by_type(pk1, "like".to_string(), 100).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 3, "json: {json}");
    }

    #[test]
    fn test_notifications_cross_account_snooping_rejected() {
        let (_pk1, _g, _s) = setup_test_context("snoop");
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let pk2 = keys2.public_key().to_hex();

        // While pk1 is the active signer, queries for pk2's notifications must fail
        let err_unread = notifications_fetch_unread(pk2.clone(), 10).unwrap_err();
        assert!(err_unread.contains("identity mismatch"), "{err_unread}");

        let err_fetch = notifications_fetch(pk2.clone(), 10, 0).unwrap_err();
        assert!(err_fetch.contains("identity mismatch"), "{err_fetch}");

        let err_by_type = notifications_fetch_by_type(pk2.clone(), "like".into(), 10).unwrap_err();
        assert!(err_by_type.contains("identity mismatch"), "{err_by_type}");

        let err_count = notifications_get_unread_count(pk2.clone()).unwrap_err();
        assert!(err_count.contains("identity mismatch"), "{err_count}");

        let err_ignored = notifications_list_ignored(pk2).unwrap_err();
        assert!(err_ignored.contains("identity mismatch"), "{err_ignored}");
    }

    #[test]
    fn test_unregister_push_requires_active_account() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let dir = soshal_test_util::tmp_root("notif_unreg");
        let db_path = dir.join("app.db").to_string_lossy().to_string();
        db::db_init(db_path.clone()).unwrap();
        // Empty session: no active account.
        session::session_load(db_path).unwrap();
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        let pk_other = soshal_nostr_core::keys::generate_keys()
            .public_key()
            .to_hex();
        let err = notifications_unregister_push(pk.clone()).unwrap_err();
        assert_eq!(err, "push token must be registered for the active account");
        // Wrong account vs the active one.
        session::session_add_account(pk.clone(), "npub1pk1valid".to_string(), "[]".to_string())
            .unwrap();
        let err = notifications_unregister_push(pk_other).unwrap_err();
        assert_eq!(err, "push token must be registered for the active account");
        assert!(notifications_unregister_push(pk).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_batch18_notifications_hardening() {
        let (pk1, _g, _s) = setup_test_context("b18_notif");
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let pk2 = keys2.public_key().to_hex();

        // Reject self-ignore
        let err = notifications_ignore_user(pk1.clone(), pk1.clone()).unwrap_err();
        assert!(err.contains("cannot ignore self"), "{err}");

        // Ignore user with casing differences
        assert!(
            notifications_ignore_user(pk1.to_ascii_uppercase(), pk2.to_ascii_uppercase()).unwrap()
        );
        assert!(notifications_is_ignored(
            pk1.clone(),
            "user".to_string(),
            pk2.clone(),
            String::new(),
        )
        .unwrap());
        assert!(notifications_unignore_user(pk1.clone(), pk2.to_ascii_uppercase()).unwrap());
        assert!(!notifications_is_ignored(
            pk1.clone(),
            "user".to_string(),
            pk2.clone(),
            String::new(),
        )
        .unwrap());
    }
}
