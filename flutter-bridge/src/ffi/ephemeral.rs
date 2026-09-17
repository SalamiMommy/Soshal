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
    if !(1..=100).contains(&max_views) {
        return Err("max_views must be between 1 and 100".to_string()).into();
    }
    if sender_pubkey.is_empty()
        || sender_pubkey.len() > 128
        || recipient_pubkey.is_empty()
        || recipient_pubkey.len() > 128
    {
        return Err("invalid pubkey length".to_string()).into();
    }
    if !soshal_common_core::url::is_valid_media_url(&media_url) {
        return Err("invalid or unsafe media URL".to_string()).into();
    }
    let caller = super::signer::signer_pubkey()?;
    if !soshal_common_core::util::constant_time_eq(caller.as_bytes(), sender_pubkey.as_bytes())
        && !soshal_common_core::util::constant_time_eq(
            caller.as_bytes(),
            recipient_pubkey.as_bytes(),
        )
    {
        return Err("identity mismatch: caller is neither sender nor recipient".to_string());
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
    let caller = super::signer::signer_pubkey()?;
    super::db::with_db_result(|db| {
        let row = EphemeralMediaRepo::new(db)
            .get(&id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !soshal_common_core::util::constant_time_eq(
            row.recipient_pubkey.as_bytes(),
            caller.as_bytes(),
        ) && !soshal_common_core::util::constant_time_eq(
            row.sender_pubkey.as_bytes(),
            caller.as_bytes(),
        ) {
            return Err(soshal_db_core::error::DbError::Migration(
                "unauthorized to view ephemeral media".to_string(),
            ));
        }
        Ok(row_json(&row))
    })
}

fn cleanup_expired_best_effort() {
    let _ = super::db::with_db_result(|db| {
        EphemeralMediaRepo::new(db).clean_expired(soshal_common_core::format::now_secs())
    });
}

/// All pending media addressed to `pubkey`, newest first, as a JSON array.
#[frb(sync, serialize)]
pub fn ephemeral_list_pending(pubkey: String) -> Result<String, String> {
    super::signer::require_identity(&pubkey)?;
    cleanup_expired_best_effort();
    super::db::with_db_result(|db| {
        let now = soshal_common_core::format::now_secs();
        let rows = EphemeralMediaRepo::new(db).get_pending_for_recipient(&pubkey)?;
        let filtered: Vec<EphemeralMediaRow> = rows
            .into_iter()
            .filter(|r| r.expires_at.map_or(true, |exp| exp > now))
            .collect();
        Ok(super::util::json_ok_or_empty(&filtered))
    })
}

/// View media: increments the counter (expiring at max_views) and returns
/// the fresh row. Errors once the media is expired.
#[frb(sync, serialize)]
pub fn ephemeral_view(id: String) -> Result<String, String> {
    let caller = super::signer::signer_pubkey()?;
    cleanup_expired_best_effort();
    super::db::with_db_result(|db| {
        let repo = EphemeralMediaRepo::new(db);
        let now = soshal_common_core::format::now_secs();
        let existing = repo
            .get(&id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        if !soshal_common_core::util::constant_time_eq(
            existing.recipient_pubkey.as_bytes(),
            caller.as_bytes(),
        ) && !soshal_common_core::util::constant_time_eq(
            existing.sender_pubkey.as_bytes(),
            caller.as_bytes(),
        ) {
            return Err(soshal_db_core::error::DbError::Migration(
                "unauthorized to view ephemeral media".to_string(),
            ));
        }
        if existing.expires_at.map_or(false, |exp| exp < now) || existing.state != "pending" {
            return Err(soshal_db_core::error::DbError::Migration(
                "ephemeral media unavailable (expired or burned)".to_string(),
            ));
        }
        let row = repo
            .increment_view_count(&id)?
            .ok_or(soshal_db_core::error::DbError::NotFound)?;
        Ok(row_json(&row))
    })
}

/// Delete a row.
#[frb(sync, serialize)]
pub fn ephemeral_delete(id: String) -> Result<bool, String> {
    let caller = super::signer::signer_pubkey()?;
    super::db::with_db_result(|db| {
        let repo = EphemeralMediaRepo::new(db);
        if let Some(existing) = repo.get(&id)? {
            if !soshal_common_core::util::constant_time_eq(
                existing.recipient_pubkey.as_bytes(),
                caller.as_bytes(),
            ) && !soshal_common_core::util::constant_time_eq(
                existing.sender_pubkey.as_bytes(),
                caller.as_bytes(),
            ) {
                return Err(soshal_db_core::error::DbError::Migration(
                    "unauthorized to delete ephemeral media".to_string(),
                ));
            }
            repo.delete(&id)?;
            Ok(true)
        } else {
            Ok(false)
        }
    })
}

/// Purge rows whose expiry is in the past; returns removed ids.
#[frb(sync, serialize)]
pub fn ephemeral_clean_expired() -> Result<Vec<String>, String> {
    super::db::with_db_result(|db| {
        EphemeralMediaRepo::new(db).clean_expired(soshal_common_core::format::now_secs())
    })
}

use super::util::uuid_like;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    fn tmp_db(label: &str) -> String {
        db::tmp_db(label, "eph")
    }

    fn save_media(
        sender_keys: &nostr::key::Keys,
        message_id: &str,
        recipient: &str,
        max_views: i64,
        expires_at: i64,
    ) -> String {
        super::super::signer::signer_unlock(sender_keys.secret_key().to_secret_hex()).unwrap();
        ephemeral_save(
            message_id.to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "https://example.com/media.jpg".to_string(),
            "image/jpeg".to_string(),
            sender_keys.public_key().to_hex(),
            recipient.to_string(),
            max_views,
            expires_at,
        )
        .unwrap()
    }

    #[test]
    fn test_save_get_roundtrip() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("roundtrip");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let id = save_media(&sender_keys, "msg1", "pk1", 5, 0);
        let json = ephemeral_get(id.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], id, "json: {json}");
        assert_eq!(v["message_id"], "msg1");
        assert_eq!(v["recipient_pubkey"], "pk1");
        assert_eq!(v["max_views"], 5);
        assert_eq!(v["current_views"], 0);
        assert_eq!(v["state"], "pending");
        assert!(v["expires_at"].is_null());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_save_rejects_zero_max_views() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("maxviews");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(sender_keys.secret_key().to_secret_hex()).unwrap();
        let res = ephemeral_save(
            "msg_bad".to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "https://example.com/media.jpg".to_string(),
            "image/jpeg".to_string(),
            sender_keys.public_key().to_hex(),
            "pk1".to_string(),
            0,
            0,
        );
        assert!(res.is_err());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_get_missing_errors() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("getmiss");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(ephemeral_get("nope".to_string()).is_err());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_list_pending_filters_recipient_and_state() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("pending");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let recip_keys = soshal_nostr_core::keys::generate_keys();
        let pk1 = recip_keys.public_key().to_hex();
        let id1 = save_media(&sender_keys, "msg1", &pk1, 2, 0);
        let id2 = save_media(&sender_keys, "msg2", &pk1, 1, 0);
        save_media(&sender_keys, "msg3", "pk2", 1, 0);

        super::super::signer::signer_unlock(recip_keys.secret_key().to_secret_hex()).unwrap();
        let json = ephemeral_list_pending(pk1.clone()).unwrap();
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
        let json = ephemeral_list_pending(pk1).unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 1, "expired row should leave pending list");
        assert_eq!(arr[0]["id"], id2);
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_view_increments_and_expires_at_max() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("view");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let recip_keys = soshal_nostr_core::keys::generate_keys();
        let pk1 = recip_keys.public_key().to_hex();
        let id = save_media(&sender_keys, "msg1", &pk1, 2, 0);

        super::super::signer::signer_unlock(recip_keys.secret_key().to_secret_hex()).unwrap();
        let json = ephemeral_view(id.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["current_views"], 1);
        assert_eq!(v["state"], "pending");
        assert!(v["viewed_at"].is_number());
        let json = ephemeral_view(id).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["current_views"], 2);
        assert_eq!(v["state"], "expired");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_view_missing_errors() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("viewmiss");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(ephemeral_view("nope".to_string()).is_err());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_clean_expired() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("clean");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let now = soshal_common_core::format::now_secs();
        let past = save_media(&sender_keys, "msg_past", "pk1", 5, now - 100);
        let future = save_media(&sender_keys, "msg_future", "pk1", 5, now + 10_000);
        let never = save_media(&sender_keys, "msg_never", "pk1", 5, 0);
        let removed = ephemeral_clean_expired().unwrap();
        assert_eq!(removed, vec![past.clone()]);
        assert!(ephemeral_get(past).is_err());
        assert!(ephemeral_get(future).is_ok());
        assert!(ephemeral_get(never).is_ok());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_delete_removes_row() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("delete");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let id = save_media(&sender_keys, "msg1", "pk1", 5, 0);
        assert!(ephemeral_delete(id.clone()).unwrap());
        assert!(ephemeral_get(id).is_err());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_empty_store() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("empty");
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let json = ephemeral_list_pending(keys.public_key().to_hex()).unwrap();
        assert_eq!(json, "[]");
        assert!(ephemeral_clean_expired().unwrap().is_empty());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_unauthorized_ephemeral_rejected() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("unauthorized");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let recip_keys = soshal_nostr_core::keys::generate_keys();
        let third_party = soshal_nostr_core::keys::generate_keys();
        let id = save_media(
            &sender_keys,
            "msg_secret",
            &recip_keys.public_key().to_hex(),
            1,
            0,
        );

        // Third party cannot view or get the ephemeral message
        super::super::signer::signer_unlock(third_party.secret_key().to_secret_hex()).unwrap();
        assert!(ephemeral_get(id.clone()).is_err());
        assert!(ephemeral_view(id.clone()).is_err());
        assert!(ephemeral_list_pending(recip_keys.public_key().to_hex()).is_err());
        assert!(ephemeral_delete(id.clone()).is_err());

        // Recipient can view it
        super::super::signer::signer_unlock(recip_keys.secret_key().to_secret_hex()).unwrap();
        assert!(ephemeral_view(id).is_ok());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_locked_signer_ephemeral_rejected() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("locked_signer");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let recip_keys = soshal_nostr_core::keys::generate_keys();
        let id = save_media(
            &sender_keys,
            "msg_locked",
            &recip_keys.public_key().to_hex(),
            1,
            0,
        );

        // Lock the signer explicitly
        super::super::signer::signer_lock().unwrap();

        // Every operation requiring authorization must fail when signer is locked
        assert!(ephemeral_get(id.clone()).is_err());
        assert!(ephemeral_view(id.clone()).is_err());
        assert!(ephemeral_list_pending(recip_keys.public_key().to_hex()).is_err());
        assert!(ephemeral_delete(id).is_err());
        assert!(ephemeral_save(
            "msg_locked2".to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "https://example.com/media.jpg".to_string(),
            "image/jpeg".to_string(),
            sender_keys.public_key().to_hex(),
            recip_keys.public_key().to_hex(),
            1,
            0,
        )
        .is_err());
    }

    #[test]
    fn test_ephemeral_save_rejects_invalid_media_url() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("bad_url");
        let sender_keys = soshal_nostr_core::keys::generate_keys();
        let recip_keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(sender_keys.secret_key().to_secret_hex()).unwrap();

        assert!(ephemeral_save(
            "m1".to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "http://127.0.0.1:8080/evil.jpg".to_string(),
            "image/jpeg".to_string(),
            sender_keys.public_key().to_hex(),
            recip_keys.public_key().to_hex(),
            1,
            0,
        )
        .is_err());

        assert!(ephemeral_save(
            "m2".to_string(),
            "conv1".to_string(),
            "dm".to_string(),
            "javascript:evil()".to_string(),
            "image/jpeg".to_string(),
            sender_keys.public_key().to_hex(),
            recip_keys.public_key().to_hex(),
            1,
            0,
        )
        .is_err());

        super::super::signer::signer_lock().unwrap();
    }
}
