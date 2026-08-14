//! Ephemeral (burn) media FFI module
//!
//! Disappearing DM media: rows in the dedicated `ephemeral_media` table with
//! a view-count cap. Port of the legacy `messaging/ephemeral.rs` commands.
//! Media bytes stay in the media CAS; rows only track metadata + lifecycle.

use flutter_rust_bridge::frb;
use soshal_db_core::repos::ephemeral_media::{EphemeralMediaRepo, EphemeralMediaRow};

/// Save an ephemeral media row. `expires_at` is a unix timestamp or 0.
#[allow(clippy::too_many_arguments)]
#[frb(sync, serialize)]
pub fn ephemeral_save(
    message_id: String,
    conversation_id: String,
    conversation_type: String,
    media_url: String,
    media_type: String,
    sender_pubkey: String,
    recipient_pubkey: String,
    max_views: i64,
    expires_at: i64,
) -> Result<String, String> {
    if max_views < 1 {
        return Err("max_views must be at least 1".to_string()).into();
    }
    let id = uuid_like();
    let now = soshal_common_core::format::now_secs();
    let row = EphemeralMediaRow {
        id: id.clone(),
        message_id,
        conversation_id,
        conversation_type,
        media_url,
        media_type,
        sender_pubkey,
        recipient_pubkey,
        max_views,
        current_views: 0,
        state: "pending".to_string(),
        expires_at: if expires_at > 0 {
            Some(expires_at)
        } else {
            None
        },
        created_at: now,
        viewed_at: None,
    };
    super::db::with_db_result(|db| {
        EphemeralMediaRepo::new(db).create(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

fn row_json(row: &EphemeralMediaRow) -> String {
    serde_json::to_string(row).unwrap_or_else(|_| "{}".to_string())
}

/// Get one row by id.
#[frb(sync, serialize)]
pub fn ephemeral_get(id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let row = EphemeralMediaRepo::new(db)
            .get(&id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        Ok(row_json(&row))
    })
}

/// All pending media addressed to `pubkey`, newest first, as a JSON array.
#[frb(sync, serialize)]
pub fn ephemeral_list_pending(pubkey: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let rows = EphemeralMediaRepo::new(db).get_pending_for_recipient(&pubkey)?;
        Ok(serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string()))
    })
}

/// View media: increments the counter (expiring at max_views) and returns
/// the fresh row. Errors once the media is expired.
#[frb(sync, serialize)]
pub fn ephemeral_view(id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let repo = EphemeralMediaRepo::new(db);
        repo.increment_view_count(&id)?;
        let row = repo
            .get(&id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        Ok(row_json(&row))
    })
}

/// Delete a row.
#[frb(sync, serialize)]
pub fn ephemeral_delete(id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        EphemeralMediaRepo::new(db).delete(&id)?;
        Ok(true)
    })
}

/// Purge rows whose expiry is in the past; returns removed ids.
#[frb(sync, serialize)]
pub fn ephemeral_clean_expired() -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| {
        EphemeralMediaRepo::new(db).clean_expired(soshal_common_core::format::now_secs())
    })
}

fn uuid_like() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;
    use std::sync::Mutex;

    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn tmp_db(label: &str) -> String {
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = format!(
            "{}/soshal_eph_{label}_{}_{}.db",
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

    fn save_media(message_id: &str, recipient: &str, max_views: i64, expires_at: i64) -> String {
        ephemeral_save(
            message_id.to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "https://example.com/media.jpg".to_string(),
            "image/jpeg".to_string(),
            "sender".to_string(),
            recipient.to_string(),
            max_views,
            expires_at,
        )
        .unwrap()
    }

    #[test]
    fn test_save_get_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("roundtrip");
        let id = save_media("msg1", "pk1", 5, 0);
        let json = ephemeral_get(id.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], id, "json: {json}");
        assert_eq!(v["message_id"], "msg1");
        assert_eq!(v["recipient_pubkey"], "pk1");
        assert_eq!(v["max_views"], 5);
        assert_eq!(v["current_views"], 0);
        assert_eq!(v["state"], "pending");
        assert!(v["expires_at"].is_null());
    }

    #[test]
    fn test_save_rejects_zero_max_views() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("maxviews");
        let res = ephemeral_save(
            "msg_bad".to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "https://example.com/media.jpg".to_string(),
            "image/jpeg".to_string(),
            "sender".to_string(),
            "pk1".to_string(),
            0,
            0,
        );
        assert!(res.is_err());
    }

    #[test]
    fn test_get_missing_errors() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("getmiss");
        assert!(ephemeral_get("nope".to_string()).is_err());
    }

    #[test]
    fn test_list_pending_filters_recipient_and_state() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("pending");
        let id1 = save_media("msg1", "pk1", 2, 0);
        let id2 = save_media("msg2", "pk1", 1, 0);
        save_media("msg3", "pk2", 1, 0);
        let json = ephemeral_list_pending("pk1".to_string()).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let ids: Vec<&str> = arr.iter().map(|v| v["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), 2, "json: {json}");
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id2.as_str()));
        ephemeral_view(id1.clone()).unwrap();
        ephemeral_view(id1).unwrap();
        let json = ephemeral_list_pending("pk1".to_string()).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1, "expired row should leave pending list");
        assert_eq!(arr[0]["id"], id2);
    }

    #[test]
    fn test_view_increments_and_expires_at_max() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("view");
        let id = save_media("msg1", "pk1", 2, 0);
        let json = ephemeral_view(id.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["current_views"], 1);
        assert_eq!(v["state"], "pending");
        assert!(v["viewed_at"].is_number());
        let json = ephemeral_view(id).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["current_views"], 2);
        assert_eq!(v["state"], "expired");
    }

    #[test]
    fn test_view_missing_errors() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("viewmiss");
        assert!(ephemeral_view("nope".to_string()).is_err());
    }

    #[test]
    fn test_clean_expired() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("clean");
        let now = soshal_common_core::format::now_secs();
        let past = save_media("msg_past", "pk1", 5, now - 100);
        let future = save_media("msg_future", "pk1", 5, now + 10_000);
        let never = save_media("msg_never", "pk1", 5, 0);
        let removed = ephemeral_clean_expired().unwrap();
        assert_eq!(removed, vec![past.clone()]);
        assert!(ephemeral_get(past).is_err());
        assert!(ephemeral_get(future).is_ok());
        assert!(ephemeral_get(never).is_ok());
    }

    #[test]
    fn test_delete_removes_row() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("delete");
        let id = save_media("msg1", "pk1", 5, 0);
        assert!(ephemeral_delete(id.clone()).unwrap());
        assert!(ephemeral_get(id).is_err());
    }

    #[test]
    fn test_empty_store() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = tmp_db("empty");
        let json = ephemeral_list_pending("pk1".to_string()).unwrap();
        assert_eq!(json, "[]");
        assert!(ephemeral_clean_expired().unwrap().is_empty());
    }
}
