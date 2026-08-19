//! Guestbook: signed kind-30080 entries targeted at a profile owner, plus
//! kind-30081 owner approvals. Entries persist in `guestbook_entries`
//! (db-core) and mirror to relays via the sync outbox.

use flutter_rust_bridge::frb;
use serde_json::json;

const KIND_GUESTBOOK: u16 = soshal_common_core::consts::KIND_GUESTBOOK;
const KIND_GUESTBOOK_APPROVAL: u16 = soshal_common_core::consts::KIND_GUESTBOOK_APPROVAL;
const MAX_CONTENT_LEN: usize = 2_000;

fn hex64(v: &str) -> bool {
    v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit())
}

fn entry_json(row: &soshal_db_core::repos::guestbook::GuestbookEntryRow) -> serde_json::Value {
    json!({
        "id": row.id,
        "pubkey": row.sender_pubkey,
        "profilePubkey": row.profile_pubkey,
        "name": row.sender_name,
        "avatar": row.sender_avatar,
        "content": row.content,
        "createdAt": row.created_at,
        "sig": row.signature,
        "approved": row.approved,
    })
}

fn sender_name_of(sender_pubkey: &str) -> Option<String> {
    let profile_json = super::identity::identity_get_profile(sender_pubkey.to_string()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&profile_json).ok()?;
    let name = v["name"].as_str()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Sign + store + relay a guestbook entry (kind 30080, `p` = owner pubkey).
/// Returns the stored entry JSON. Requires the unlocked signer.
#[frb(serialize)]
pub async fn guestbook_add(profile_pubkey: String, content: String) -> Result<String, String> {
    if !hex64(&profile_pubkey) {
        return Err("invalid profile pubkey".to_string()).into();
    }
    if content.is_empty() || content.len() > MAX_CONTENT_LEN {
        return Err(format!("content must be 1..={MAX_CONTENT_LEN} chars")).into();
    }
    let sender_pubkey = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::Custom(KIND_GUESTBOOK),
        content.clone(),
    )
    .tags(vec![nostr::event::Tag::parse(vec!["p", &profile_pubkey])
        .map_err(|e| format!("tag build failed: {e}"))?]);
    let signed_json = super::signer::sign_builder(builder)?;
    let signed: serde_json::Value =
        serde_json::from_str(&signed_json).map_err(|e| format!("signed event is not json: {e}"))?;
    let id = signed["id"]
        .as_str()
        .ok_or_else(|| "signed event has no id".to_string())?
        .to_string();
    let created_at = signed["created_at"]
        .as_i64()
        .ok_or_else(|| "signed event has no created_at".to_string())?;
    let row = soshal_db_core::repos::guestbook::GuestbookEntryRow {
        id: id.clone(),
        profile_pubkey: profile_pubkey.clone(),
        sender_name: sender_name_of(&sender_pubkey),
        sender_pubkey,
        sender_avatar: None,
        content,
        created_at,
        signature: signed["sig"].as_str().map(|s| s.to_string()),
        approved: false,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db).insert(&row)
    })?;
    super::sync::publish_or_enqueue("guestbook", &signed_json).await?;
    serde_json::to_string(&entry_json(&row)).map_err(|e| e.to_string())
}

/// List guestbook entries for a profile pubkey from the local DB.
#[frb(sync, serialize)]
pub fn guestbook_list(
    profile_pubkey: String,
    limit: i32,
    only_approved: bool,
) -> Result<String, String> {
    super::db::with_db_string(|db| {
        let rows = soshal_db_core::repos::guestbook::GuestbookRepo::new(db)
            .list_by_profile(&profile_pubkey, i64::from(limit), only_approved)
            .map_err(|e| e.to_string())?;
        let arr: Vec<serde_json::Value> = rows.iter().map(entry_json).collect();
        serde_json::to_string(&arr).map_err(|e| e.to_string())
    })
}

/// Owner approval/denial: sign kind 30081 (`e` entry, `p` owner), update the
/// local row, relay the decision. Only the profile owner may approve.
#[frb(serialize)]
pub async fn guestbook_approve(entry_id: String, approved: bool) -> Result<String, String> {
    if entry_id.len() != 64 || !entry_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid entry id".to_string()).into();
    }
    let owner_pubkey = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let target = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let sql = "SELECT profile_pubkey FROM guestbook_entries WHERE id = ?";
        let v = soshal_db_core::query::query_first(&conn, sql, [entry_id.as_str()], |r| {
            let s: String = r.get(0)?;
            Ok(s)
        })?;
        Ok(v)
    })?;
    let Some(profile_pubkey) = target else {
        return Err("guestbook entry not found".to_string()).into();
    };
    if profile_pubkey != owner_pubkey {
        return Err("only the profile owner can approve entries".to_string()).into();
    }
    let content = if approved { "approved" } else { "rejected" };
    let builder = nostr::event::EventBuilder::new(
        nostr::event::Kind::Custom(KIND_GUESTBOOK_APPROVAL),
        content.to_string(),
    )
    .tags(vec![
        nostr::event::Tag::parse(vec!["e", &entry_id])
            .map_err(|e| format!("tag build failed: {e}"))?,
        nostr::event::Tag::parse(vec!["p", &profile_pubkey])
            .map_err(|e| format!("tag build failed: {e}"))?,
    ]);
    let signed_json = super::signer::sign_builder(builder)?;
    super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db).set_approved(&entry_id, approved)
    })?;
    super::sync::publish_or_enqueue("guestbook_approval", &signed_json).await?;
    Ok(signed_json).into()
}

/// Owner-only local delete of a guestbook entry.
#[frb(sync, serialize)]
pub fn guestbook_delete(entry_id: String) -> Result<bool, String> {
    if entry_id.len() != 64 || !entry_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid entry id".to_string()).into();
    }
    let owner_pubkey = match super::signer::signer_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("signer locked".to_string()).into(),
    };
    let profile_pubkey = super::db::with_db_result(|db| {
        let conn = db.conn()?;
        let sql = "SELECT profile_pubkey FROM guestbook_entries WHERE id = ?";
        let v = soshal_db_core::query::query_first(&conn, sql, [entry_id.as_str()], |r| {
            let s: String = r.get(0)?;
            Ok(s)
        })?;
        Ok(v)
    })?;
    let Some(pk) = profile_pubkey else {
        return Ok(false).into();
    };
    if pk != owner_pubkey {
        return Err("only the profile owner can delete entries".to_string()).into();
    }
    super::db::with_db_result(|db| {
        soshal_db_core::repos::guestbook::GuestbookRepo::new(db).delete(&entry_id)
    })?;
    Ok(true).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex64() {
        assert!(hex64(&"a".repeat(64)));
        assert!(!hex64(&"a".repeat(63)));
        assert!(!hex64(&"zz".repeat(32)));
    }

    #[test]
    fn test_entry_json_shape() {
        let row = soshal_db_core::repos::guestbook::GuestbookEntryRow {
            id: "e".repeat(64),
            profile_pubkey: "p".repeat(64),
            sender_pubkey: "s".repeat(64),
            sender_name: Some("Alice".to_string()),
            sender_avatar: None,
            content: "hi".to_string(),
            created_at: 42,
            signature: Some("sig".to_string()),
            approved: false,
        };
        let v = entry_json(&row);
        assert_eq!(v["id"], "e".repeat(64));
        assert_eq!(v["pubkey"], "s".repeat(64));
        assert_eq!(v["name"], "Alice");
        assert_eq!(v["approved"], false);
        assert_eq!(v["createdAt"], 42);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_guestbook_add_list_approve_delete() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = crate::ffi::db::tmp_db("guestbook", "gb");
        let guest = nostr::key::Keys::generate();
        let owner_keys = nostr::key::Keys::generate();
        let owner = owner_keys.public_key().to_hex();
        super::super::signer::signer_unlock(guest.secret_key().to_secret_hex()).unwrap();

        let entry_json_str = guestbook_add(owner.clone(), "nice profile!".to_string())
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&entry_json_str).unwrap();
        let entry_id = v["id"].as_str().unwrap().to_string();
        assert_eq!(v["pubkey"], guest.public_key().to_hex());
        assert_eq!(v["approved"], false);

        let list = guestbook_list(owner.clone(), 50, false).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 1);
        // Unapproved hidden from public view.
        let public = guestbook_list(owner.clone(), 50, true).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&public).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 0);

        // Guest cannot approve; only the owner can.
        assert!(guestbook_approve(entry_id.clone(), true).await.is_err());
        super::super::signer::signer_lock().unwrap();
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        let approval = guestbook_approve(entry_id.clone(), true).await.unwrap();
        assert!(approval.contains("approved"));
        let public = guestbook_list(owner.clone(), 50, true).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&public).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 1);

        // Non-owner cannot delete.
        super::super::signer::signer_lock().unwrap();
        super::super::signer::signer_unlock(guest.secret_key().to_secret_hex()).unwrap();
        assert!(guestbook_delete(entry_id.clone()).is_err());

        // Owner delete works.
        super::super::signer::signer_lock().unwrap();
        super::super::signer::signer_unlock(owner_keys.secret_key().to_secret_hex()).unwrap();
        assert!(guestbook_delete(entry_id.clone()).unwrap());
        let list = guestbook_list(owner, 50, false).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 0);
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_guestbook_add_rejects_bad_input() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _p = crate::ffi::db::tmp_db("guestbook-bad", "gb");
        let keys = nostr::key::Keys::generate();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(guestbook_add("not-hex".to_string(), "hi".to_string())
            .await
            .is_err());
        assert!(guestbook_add("f".repeat(64), String::new()).await.is_err());
        assert!(guestbook_add("f".repeat(64), "x".repeat(2_001))
            .await
            .is_err());
        super::super::signer::signer_lock().unwrap();
    }
}
