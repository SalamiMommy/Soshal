//! Bookmarks FFI module
//!
//! Thin wrapper over db-core's `BookmarkRepo` (local bookmark table).

use flutter_rust_bridge::frb;
use nostr::event::{EventBuilder, Kind, Tag};

/// Save a bookmark for an event. Returns the bookmark id (event id).
#[frb(sync, serialize)]
pub fn bookmarks_save(pubkey: String, event_id: String) -> Result<String, String> {
    super::signer::require_identity(&pubkey)?;
    let now = soshal_common_core::format::now_secs();
    let row = soshal_db_core::repos::bookmark::BookmarkRow {
        id: format!("bm:{pubkey}:{event_id}"),
        pubkey,
        event_id,
        created_at: now,
    };
    super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    publish_bookmark_list(&row.pubkey);
    Ok(row.id).into()
}

/// List bookmarks for a pubkey, newest first. Returns JSON array of
/// `{id, pubkey, event_id, created_at}`.
#[frb(sync, serialize)]
pub fn bookmarks_list(pubkey: String, limit: i64, offset: i64) -> Result<String, String> {
    super::signer::require_identity(&pubkey)?;
    super::db::with_db_result(|db| {
        let rows = soshal_db_core::repos::bookmark::BookmarkRepo::new(db)
            .get_user_bookmarks(&pubkey, limit, offset)?;
        Ok(rows)
    })
    .and_then(super::util::json_ok)
}

/// Delete a bookmark by id. Returns true if a row was removed.
#[frb(sync, serialize)]
pub fn bookmarks_delete(id: String) -> Result<bool, String> {
    let row = super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::bookmark::BookmarkRepo::new(db);
        if let Some(r) = repo.get_by_id(&id)? {
            return Ok(Some(r));
        }
        let target_evt = id.strip_prefix("bm:").unwrap_or(&id);
        let conn = db.conn()?;
        if let Ok(active_pk) = super::signer::signer_pubkey() {
            let user_bm_id = format!("bm:{active_pk}:{target_evt}");
            if let Some(r) = repo.get_by_id(&user_bm_id)? {
                return Ok(Some(r));
            }
            let found: Option<String> = soshal_db_core::query::query_first(
                &conn,
                "SELECT id FROM bookmarks WHERE pubkey = ?1 AND (id = ?2 OR event_id = ?3)",
                libsql::params![active_pk.as_str(), id.as_str(), target_evt],
                |r| r.get(0),
            )?;
            if let Some(exact_id) = found {
                return repo.get_by_id(&exact_id);
            }
        }
        let found: Option<String> = soshal_db_core::query::query_first(
            &conn,
            "SELECT id FROM bookmarks WHERE id = ?1 OR event_id = ?2",
            libsql::params![id.as_str(), target_evt],
            |r| r.get(0),
        )?;
        if let Some(exact_id) = found {
            return repo.get_by_id(&exact_id);
        }
        Ok(None)
    })?;
    if let Some(r) = row {
        super::signer::require_identity(&r.pubkey)?;
        super::db::with_db_result(|db| {
            soshal_db_core::repos::bookmark::BookmarkRepo::new(db).delete(&r.id)?;
            Ok(())
        })?;
        publish_bookmark_list(&r.pubkey);
    }
    Ok(true)
}

/// Resolve a bookmarked event from the local DB cache.
/// Returns JSON of PostRow or empty string if not found.
#[frb(sync, serialize)]
pub fn bookmarks_resolve_post(event_id: String) -> Result<String, String> {
    super::db::with_db_result(|db| {
        let row = soshal_db_core::repos::post::PostRepo::new(db).get_by_id(&event_id)?;
        Ok(row)
    })
    .and_then(|r| match r {
        Some(row) if !row.is_deleted => super::util::json_ok(row),
        _ => Ok(String::new()),
    })
}

/// Resolve many bookmarked events in one call. `ids_json` is a JSON array of
/// event ids; returns a JSON map `{"<id>": PostRow, ...}` (missing rows are
/// absent). Replaces N sequential per-bookmark FFI round-trips.
#[frb(sync, serialize)]
pub fn bookmarks_resolve_posts(ids_json: String) -> Result<String, String> {
    let ids: Vec<String> =
        serde_json::from_str(&ids_json).map_err(|e| format!("invalid ids JSON: {e}"))?;
    if ids.is_empty() {
        return Ok("{}".to_string());
    }
    super::db::with_db_result(|db| {
        let repo = soshal_db_core::repos::post::PostRepo::new(db);
        let rows = repo.get_by_ids(&ids)?;
        let map: std::collections::HashMap<&str, &soshal_db_core::repos::post::PostRow> = rows
            .iter()
            .filter(|r| !r.is_deleted)
            .map(|r| (r.id.as_str(), r))
            .collect();
        Ok(serde_json::to_string(&map).unwrap_or_else(|_| "{}".into()))
    })
}

/// Best-effort publish of the user's bookmark list (NIP-51 kind 10003) with
/// one `e` tag per saved bookmark. Signer-locked or unreachable relays leave
/// bookmarks local-only (DB write already succeeded).
fn publish_bookmark_list(pubkey: &str) {
    if super::signer::signer_pubkey().is_err() {
        return;
    }
    let rows = super::db::with_db_result(|db| {
        soshal_db_core::repos::bookmark::BookmarkRepo::new(db).get_user_bookmarks(pubkey, 1000, 0)
    })
    .unwrap_or_default();
    let mut builder = EventBuilder::new(Kind::from_u16(10003), "");
    for r in &rows {
        if let Ok(tag) = Tag::parse(vec!["e".to_string(), r.event_id.clone()]) {
            builder = builder.tag(tag);
        }
    }
    if let Ok(signed) = super::signer::sign_builder(builder) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = super::network::network_publish_event(signed).await;
            });
        } else {
            std::thread::spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    let _ = rt.block_on(super::network::network_publish_event(signed));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(label: &str) -> String {
        crate::ffi::db::tmp_db(label, "bmk")
    }

    fn insert_post(id: &str, pubkey: &str, content: &str) {
        crate::ffi::db::insert_test_user(pubkey);
        crate::ffi::db::db_execute_params(
            "INSERT INTO posts (id, pubkey, content, kind, created_at) VALUES (?1, ?2, ?3, 1, 1700000000)",
            &[id.to_string(), pubkey.to_string(), content.to_string()],
        )
        .unwrap();
    }

    #[test]
    fn test_save_list_delete_roundtrip() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("crud");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        crate::ffi::db::insert_test_user(&pk);
        let id = bookmarks_save(pk.clone(), "evt1".into()).unwrap();
        assert_eq!(id, format!("bm:{pk}:evt1"));
        bookmarks_save(pk.clone(), "evt2".into()).unwrap();
        // Newest-first ordering + offset pagination.
        let list = bookmarks_list(pk.clone(), 10, 0).unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Both bookmarks present (same-second created_at makes order a tie).
        assert!(list.contains("evt1") && list.contains("evt2"));
        let paged = bookmarks_list(pk.clone(), 1, 1).unwrap();
        let pv: serde_json::Value = serde_json::from_str(&paged).unwrap();
        assert_eq!(pv.as_array().unwrap().len(), 1);
        // Delete returns true once, and stays idempotent for a missing id.
        assert!(bookmarks_delete(id).unwrap());
        assert!(bookmarks_delete("bm:missing".into()).unwrap());
        let list = bookmarks_list(pk, 10, 0).unwrap();
        assert!(!list.contains("evt1"));
        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_bookmarks_multi_user_isolation_and_delete_auth() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("isolation");
        let keys1 = soshal_nostr_core::keys::generate_keys();
        let pk1 = keys1.public_key().to_hex();
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let pk2 = keys2.public_key().to_hex();
        crate::ffi::db::insert_test_user(&pk1);
        crate::ffi::db::insert_test_user(&pk2);

        // User 1 bookmarks evt1
        super::super::signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        let id1 = bookmarks_save(pk1.clone(), "evt1".into()).unwrap();

        // User 2 bookmarks same evt1 without overwriting User 1
        super::super::signer::signer_unlock(keys2.secret_key().to_secret_hex()).unwrap();
        let id2 = bookmarks_save(pk2.clone(), "evt1".into()).unwrap();
        assert_ne!(id1, id2);

        // User 2 cannot list User 1's private bookmarks
        let denied = bookmarks_list(pk1.clone(), 10, 0);
        assert!(denied.is_err());
        assert!(denied.unwrap_err().contains("identity mismatch"));

        let list2 = bookmarks_list(pk2.clone(), 10, 0).unwrap();
        assert!(list2.contains("evt1"));

        // User 2 cannot delete User 1's bookmark
        let denied = bookmarks_delete(id1.clone());
        assert!(denied.is_err());
        assert!(denied.unwrap_err().contains("identity mismatch"));

        // User 1 deletes their own bookmark
        super::super::signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        let list1 = bookmarks_list(pk1, 10, 0).unwrap();
        assert!(list1.contains("evt1"));
        assert!(bookmarks_delete(id1).unwrap());

        // User 2's bookmark remains intact
        super::super::signer::signer_unlock(keys2.secret_key().to_secret_hex()).unwrap();
        let list2_after = bookmarks_list(pk2, 10, 0).unwrap();
        assert!(list2_after.contains("evt1"));

        let _ = super::super::signer::signer_lock();
    }

    #[test]
    fn test_resolve_post_found_and_missing() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("resolve");
        insert_post("p1", "pk1", "hello");
        let json = bookmarks_resolve_post("p1".into()).unwrap();
        assert!(json.contains("hello"), "json: {json}");
        assert!(json.contains("\"pubkey\":\"pk1\""));
        assert_eq!(bookmarks_resolve_post("nope".into()).unwrap(), "");
    }

    #[test]
    fn test_resolve_posts_batch() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("resolve-batch");
        insert_post("p1", "pk1", "one");
        insert_post("p2", "pk2", "two");
        let out = bookmarks_resolve_posts(r#"["p1","p2","missing"]"#.to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let map = v.as_object().unwrap();
        assert_eq!(map.len(), 2, "out: {out}");
        assert_eq!(map["p1"]["content"], "one");
        assert_eq!(map["p2"]["content"], "two");
        // Empty id list → empty map.
        let out = bookmarks_resolve_posts("[]".to_string()).unwrap();
        assert_eq!(out, "{}");
        // Malformed ids JSON → error.
        assert!(bookmarks_resolve_posts("not-json".to_string()).is_err());
    }
}
