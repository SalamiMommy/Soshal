//! Scheduled posts FFI module
//!
//! Draft posts with a future `scheduled_at` timestamp, persisted in the
//! `posts` table (kind 1). Publishing at the scheduled time is the sync
//! pipeline's job (`sync` module); this module manages the drafts.

use flutter_rust_bridge::frb;
use soshal_db_core::repos::post::{PostRepo, PostRow};

fn hashtags_to_json(hashtags: &[String]) -> String {
    serde_json::to_string(hashtags).unwrap_or_else(|_| "[]".into())
}

/// Create a scheduled post draft. `scheduled_at` is a unix timestamp in the
/// future; `publish_at` (unix) marks when sync should broadcast it.
#[frb(sync, serialize)]
pub fn scheduled_create(
    pubkey: String,
    content: String,
    scheduled_at: i64,
    hashtags: Vec<String>,
) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("content must not be empty".to_string());
    }
    if scheduled_at <= soshal_common_core::format::now_secs() {
        return Err("scheduled_at must be in the future".to_string());
    }
    let now = soshal_common_core::format::now_secs();
    let id = format!("sched:{pubkey}:{scheduled_at}:{now}");
    let row = PostRow {
        id: id.clone(),
        pubkey,
        content,
        kind: 1,
        created_at: now,
        tags_json: hashtags_to_json(&hashtags),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: "[]".to_string(),
        mentioned_hashtags: hashtags_to_json(&hashtags),
        subject: None,
        sync_status: "scheduled".to_string(),
        is_deleted: false,
        scheduled_at: Some(scheduled_at),
        freenet_key: None,
        is_freenet_native: false,
        rsvp_event_id: None,
    };
    super::db::with_db_result(|db| {
        PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// List scheduled drafts for a pubkey, soonest first. Returns JSON array of
/// post rows.
#[frb(sync, serialize)]
pub fn scheduled_list(pubkey: String) -> Result<String, String> {
    super::db::with_db_result(|db| PostRepo::new(db).get_scheduled(&pubkey))
        .and_then(super::util::json_ok)
}

/// Delete a scheduled draft (soft delete).
#[frb(sync, serialize)]
pub fn scheduled_delete(id: String) -> Result<bool, String> {
    super::db::with_db_result(|db| {
        PostRepo::new(db).delete(&id)?;
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    fn tmp_db(label: &str) -> String {
        db::tmp_db(label, "sched")
    }

    fn parse_arr(json: &str) -> Vec<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn test_create_list_roundtrip() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("roundtrip");
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user("pk1");
        let id = scheduled_create(
            "pk1".to_string(),
            "hello scheduled world".to_string(),
            at,
            vec!["nostr".to_string(), "soshal".to_string()],
        )
        .unwrap();
        assert!(id.starts_with("sched:pk1:"));
        let arr = parse_arr(&scheduled_list("pk1".to_string()).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], id);
        assert_eq!(arr[0]["pubkey"], "pk1");
        assert_eq!(arr[0]["content"], "hello scheduled world");
        assert_eq!(arr[0]["kind"], 1);
        assert_eq!(arr[0]["sync_status"], "scheduled");
        assert_eq!(arr[0]["scheduled_at"], at);
        assert_eq!(arr[0]["is_deleted"], false);
        let tags_str = arr[0]["mentioned_hashtags"].as_str().unwrap();
        let tags: serde_json::Value = serde_json::from_str(tags_str).unwrap();
        let tags_arr = tags.as_array().unwrap();
        assert_eq!(tags_arr.len(), 2, "json: {arr:?}");
    }

    #[test]
    fn test_list_orders_soonest_first() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("order");
        let late = soshal_common_core::format::now_secs() + 7200;
        let early = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user("pk1");
        scheduled_create("pk1".to_string(), "later".to_string(), late, vec![]).unwrap();
        scheduled_create("pk1".to_string(), "sooner".to_string(), early, vec![]).unwrap();
        let arr = parse_arr(&scheduled_list("pk1".to_string()).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["content"], "sooner");
        assert_eq!(arr[1]["content"], "later");
    }

    #[test]
    fn test_list_filters_pubkey() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("pubkey");
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user("pk1");
        db::insert_test_user("pk2");
        scheduled_create("pk1".to_string(), "mine".to_string(), at, vec![]).unwrap();
        scheduled_create("pk2".to_string(), "theirs".to_string(), at, vec![]).unwrap();
        let arr = parse_arr(&scheduled_list("pk1".to_string()).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["content"], "mine");
    }

    #[test]
    fn test_list_empty_store() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("empty");
        let arr = parse_arr(&scheduled_list("pk1".to_string()).unwrap());
        assert!(arr.is_empty());
    }

    #[test]
    fn test_create_rejects_empty_content() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("emptymsg");
        let res = scheduled_create(
            "pk1".to_string(),
            "   ".to_string(),
            soshal_common_core::format::now_secs() + 3600,
            vec![],
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("content"));
    }

    #[test]
    fn test_create_rejects_past_timestamp() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("past");
        let res = scheduled_create(
            "pk1".to_string(),
            "too late".to_string(),
            soshal_common_core::format::now_secs() - 60,
            vec![],
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("future"));
    }

    #[test]
    fn test_delete_removes_from_list() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("delete");
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user("pk1");
        let id = scheduled_create("pk1".to_string(), "draft".to_string(), at, vec![]).unwrap();
        assert!(scheduled_delete(id).unwrap());
        let arr = parse_arr(&scheduled_list("pk1".to_string()).unwrap());
        assert!(arr.is_empty(), "json: {arr:?}");
    }

    #[test]
    fn test_delete_unknown_id_noop() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _p = tmp_db("unknown");
        assert!(scheduled_delete("sched:missing:1:2".to_string()).unwrap());
    }
}
