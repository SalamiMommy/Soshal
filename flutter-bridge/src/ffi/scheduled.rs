//! Scheduled posts FFI module
//!
//! Draft posts with a future `scheduled_at` timestamp, persisted in the
//! `posts` table (kind 1). Publishing at the scheduled time is the sync
//! pipeline's job (`sync` module); this module manages the drafts.

use flutter_rust_bridge::frb;
use soshal_db_core::repos::post::{PostRepo, PostRow};

/// How long a moderation-blocked scheduled draft is nudged forward so a page
/// full of blocked drafts can't starve later valid drafts from publishing.
const SCHEDULE_BLOCKED_RETRY_SECS: i64 = 900;

fn hashtags_to_json(hashtags: &[String]) -> String {
    serde_json::to_string(hashtags).unwrap_or_else(|_| "[]".into())
}

fn hashtags_to_nostr_tags_json(hashtags: &[String]) -> String {
    let tags: Vec<Vec<&str>> = hashtags.iter().map(|h| vec!["t", h.as_str()]).collect();
    serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into())
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
    super::signer::require_identity(&pubkey)?;
    if content.trim().is_empty() {
        return Err("content must not be empty".to_string());
    }
    if scheduled_at <= soshal_common_core::format::now_secs() {
        return Err("scheduled_at must be in the future".to_string());
    }
    let now = soshal_common_core::format::now_secs();
    // Unique draft id: nanosecond-resolution nonce so two drafts created in
    // the same second for the same scheduled_at can never collide, plus a
    // bounded retry against an id that already exists (e.g. recreated after
    // a soft delete left the row behind).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut id = format!("sched:{pubkey}:{scheduled_at}:{nanos}");
    let mut attempt = 0u32;
    loop {
        let conflict =
            super::db::with_db_result(|db| Ok(PostRepo::new(db).get_by_id(&id)?.is_some()))?;
        if !conflict {
            break;
        }
        attempt += 1;
        if attempt >= 8 {
            return Err("could not allocate a unique draft id (too many collisions)".to_string());
        }
        id = format!("sched:{pubkey}:{scheduled_at}:{nanos}-{attempt}");
    }
    let row = PostRow {
        id: id.clone(),
        pubkey,
        content,
        kind: 1,
        created_at: now,
        tags_json: hashtags_to_nostr_tags_json(&hashtags),
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
        soshal_db_core::repos::user::UserRepo::new(db).ensure_exists(&row.pubkey)?;
        PostRepo::new(db).upsert(&row)?;
        Ok(())
    })?;
    Ok(id).into()
}

/// List scheduled drafts for a pubkey, soonest first. Returns JSON array of
/// post rows.
#[frb(sync, serialize)]
pub fn scheduled_list(pubkey: String) -> Result<String, String> {
    super::signer::require_identity(&pubkey)?;
    super::db::with_db_result(|db| PostRepo::new(db).get_scheduled(&pubkey))
        .and_then(super::util::json_ok)
}

/// Delete a scheduled draft (soft delete).
#[frb(sync, serialize)]
pub fn scheduled_delete(id: String) -> Result<bool, String> {
    if let Some(rest) = id.strip_prefix("sched:") {
        if let Some((pk, _)) = rest.split_once(':') {
            super::signer::require_identity(pk)?;
        }
    }
    super::db::with_db_result(|db| {
        let repo = PostRepo::new(db);
        if let Some(post) = repo.get_by_id(&id)? {
            super::signer::require_identity(&post.pubkey)
                .map_err(|e| soshal_db_core::error::DbError::Forbidden(e))?;
            if post.scheduled_at.is_none() && post.sync_status != "scheduled" {
                return Err(soshal_db_core::error::DbError::NotFound);
            }
        }
        repo.delete(&id)?;
        Ok(true)
    })
}

/// Publish every scheduled draft whose `scheduled_at` has arrived for
/// `pubkey`. Each draft is signed as a real kind-1 event, persisted as a
/// synced post, removed from the draft set, and broadcast via the sync
/// pipeline. Returns the number of drafts published. Idempotent: only
/// rows still in `sync_status = 'scheduled'` are touched.
#[frb(serialize)]
pub async fn scheduled_publish_due(pubkey: String, limit: i32) -> Result<u32, String> {
    super::signer::require_identity(&pubkey)?;
    let limit = limit.clamp(1, 100) as i64;
    let now = soshal_common_core::format::now_secs();
    let drafts =
        super::db::with_db_result(|db| PostRepo::new(db).get_due_scheduled(&pubkey, now, limit))?;
    let mut published = 0u32;
    for mut draft in drafts {
        let content = draft.content.trim().to_string();
        if content.is_empty() {
            continue; // empty draft: skip, stays scheduled
        }
        soshal_feed_core::publish::validate_note_content(&content).map_err(super::util::to_err)?;
        let filters = super::db::with_db_result(|db| Ok(super::feed::get_custom_word_filters(db)))
            .unwrap_or_default();
        let verdict = soshal_moderation_core::check::check_with_custom_words(&content, &filters);
        if !verdict.passed {
            // Blocked draft: keep the row (stays scheduled so the user can
            // edit/delete it) instead of aborting the whole due-batch loop —
            // but nudge its schedule forward so a page full of blocked
            // drafts can't starve later valid drafts (get_due_scheduled
            // returns the first N due drafts ordered by scheduled_at).
            draft.scheduled_at = Some(now + SCHEDULE_BLOCKED_RETRY_SECS);
            let _ = super::db::with_db_result(|db| PostRepo::new(db).upsert(&draft));
            eprintln!(
                "scheduled publish skipped (moderation blocked, retry in {}s): {} {:?}",
                SCHEDULE_BLOCKED_RETRY_SECS, draft.id, verdict.reason
            );
            continue;
        }
        // tags: stored Nostr-format tags_json + hashtags extracted from
        // content (deduped, case-insensitive) so `#x` typed but unlisted
        // survives publish.
        let mut tags: Vec<Vec<String>> = serde_json::from_str(&draft.tags_json).unwrap_or_default();
        let mut seen: std::collections::HashSet<String> = tags
            .iter()
            .filter(|t| t.first().map(|k| k == "t").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .map(|t| t.to_ascii_lowercase())
            .collect();
        for tag in soshal_content_core::hashtag::extract(&content) {
            if seen.insert(tag.to_ascii_lowercase()) {
                tags.push(vec!["t".to_string(), tag]);
            }
        }
        let builder = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, &content).tags(
            tags.iter()
                .filter_map(|t| nostr::event::Tag::parse(t.clone()).ok()),
        );
        let signed_json = super::signer::sign_builder(builder)?;
        let signed: SignedScheduledEvent = serde_json::from_str(&signed_json)
            .map_err(|e| format!("signer returned malformed event: {e}"))?;
        let created_at = signed
            .created_at
            .unwrap_or_else(soshal_common_core::format::now_secs);
        let tags_json = serde_json::to_string(&tags).map_err(|e| format!("tags serialize: {e}"))?;
        let hashtags: Vec<String> = soshal_content_core::hashtag::extract(&content)
            .into_iter()
            .collect();
        // Publish (or enqueue into the persistent offline outbox) BEFORE
        // promoting the draft row. If this fails, `?` aborts and the draft
        // stays `sync_status='scheduled'` so the next scheduled run retries
        // it — previously the row was upserted as `synced` + the draft
        // deleted first, so a queue failure left a post that looked sent but
        // was never published (lost post).
        super::sync::publish_or_enqueue("post", &signed_json).await?;
        if let Ok(rows) = serde_json::to_string(&[ScheduledIndexRow {
            id: signed.id,
            pubkey: signed.pubkey,
            content: signed.content,
            kind: 1u64,
        }]) {
            let _ = super::search::search_index_posts(rows);
        }
        super::db::with_db_result(|db| {
            let _ = soshal_db_core::repos::user::UserRepo::new(db).ensure_exists(signed.pubkey);
            PostRepo::new(db).upsert(&PostRow {
                id: signed.id.to_string(),
                pubkey: signed.pubkey.to_string(),
                content: signed.content.to_string(),
                kind: 1,
                created_at,
                tags_json: tags_json.clone(),
                sig: Some(signed.sig.to_string()),
                reply_to: None,
                root_id: None,
                mentioned_pubkeys: String::new(),
                mentioned_hashtags: hashtags.join(","),
                subject: None,
                sync_status: "synced".to_string(),
                is_deleted: false,
                scheduled_at: None,
                freenet_key: None,
                is_freenet_native: false,
                rsvp_event_id: None,
            })?;
            // Draft row no longer referenced once the real event exists.
            PostRepo::new(db).delete(&draft.id)?;
            Ok(())
        })?;
        published += 1;
    }
    Ok(published).into()
}

/// Publish-time snapshot of the signed event header (mirrors
/// `feed::SignedEventHeader`; kept local to avoid cross-module deps).
#[derive(serde::Deserialize)]
struct SignedScheduledEvent<'a> {
    id: &'a str,
    pubkey: &'a str,
    content: &'a str,
    #[serde(default)]
    created_at: Option<i64>,
    sig: &'a str,
}

#[derive(serde::Serialize)]
struct ScheduledIndexRow<'a> {
    id: &'a str,
    pubkey: &'a str,
    content: &'a str,
    kind: u64,
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
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("roundtrip");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user(&pk);
        let id = scheduled_create(
            pk.clone(),
            "hello scheduled world".to_string(),
            at,
            vec!["nostr".to_string(), "soshal".to_string()],
        )
        .unwrap();
        assert!(id.starts_with(&format!("sched:{pk}:")));
        let arr = parse_arr(&scheduled_list(pk.clone()).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["id"], id);
        assert_eq!(arr[0]["pubkey"], pk);
        assert_eq!(arr[0]["content"], "hello scheduled world");
        assert_eq!(arr[0]["kind"], 1);
        assert_eq!(arr[0]["sync_status"], "scheduled");
        assert_eq!(arr[0]["scheduled_at"], at);
        assert_eq!(arr[0]["is_deleted"], false);
        let tags_str = arr[0]["mentioned_hashtags"].as_str().unwrap();
        let tags: serde_json::Value = serde_json::from_str(tags_str).unwrap();
        let tags_arr = tags.as_array().unwrap();
        assert_eq!(tags_arr.len(), 2, "json: {arr:?}");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_list_orders_soonest_first() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("order");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let late = soshal_common_core::format::now_secs() + 7200;
        let early = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user(&pk);
        scheduled_create(pk.clone(), "later".to_string(), late, vec![]).unwrap();
        scheduled_create(pk.clone(), "sooner".to_string(), early, vec![]).unwrap();
        let arr = parse_arr(&scheduled_list(pk).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        assert_eq!(arr[0]["content"], "sooner");
        assert_eq!(arr[1]["content"], "later");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_list_filters_pubkey() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("pubkey");
        let keys1 = soshal_nostr_core::keys::generate_keys();
        let pk1 = keys1.public_key().to_hex();
        let keys2 = soshal_nostr_core::keys::generate_keys();
        let pk2 = keys2.public_key().to_hex();
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user(&pk1);
        db::insert_test_user(&pk2);

        super::super::signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        scheduled_create(pk1.clone(), "mine".to_string(), at, vec![]).unwrap();
        super::super::signer::signer_unlock(keys2.secret_key().to_secret_hex()).unwrap();
        scheduled_create(pk2, "theirs".to_string(), at, vec![]).unwrap();

        super::super::signer::signer_unlock(keys1.secret_key().to_secret_hex()).unwrap();
        let arr = parse_arr(&scheduled_list(pk1).unwrap());
        assert_eq!(arr.len(), 1, "json: {arr:?}");
        assert_eq!(arr[0]["content"], "mine");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_list_empty_store() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("empty");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let arr = parse_arr(&scheduled_list(pk).unwrap());
        assert!(arr.is_empty());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_create_rejects_empty_content() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("emptymsg");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let res = scheduled_create(
            pk,
            "   ".to_string(),
            soshal_common_core::format::now_secs() + 3600,
            vec![],
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("content"));
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_create_rejects_past_timestamp() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("past");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let res = scheduled_create(
            pk,
            "too late".to_string(),
            soshal_common_core::format::now_secs() - 60,
            vec![],
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("future"));
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_delete_removes_from_list() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("delete");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user(&pk);
        let id = scheduled_create(pk.clone(), "draft".to_string(), at, vec![]).unwrap();
        assert!(scheduled_delete(id).unwrap());
        let arr = parse_arr(&scheduled_list(pk).unwrap());
        assert!(arr.is_empty(), "json: {arr:?}");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_delete_unknown_id_noop() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("unknown");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        assert!(scheduled_delete(format!("sched:{pk}:1:2")).unwrap());
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_scheduled_unauthorized_rejected() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        super::super::signer::signer_lock().unwrap();
        let at = soshal_common_core::format::now_secs() + 3600;
        let pk = "deadbeef".repeat(8);
        assert!(scheduled_create(pk.clone(), "test".to_string(), at, vec![]).is_err());
        assert!(scheduled_list(pk.clone()).is_err());
        assert!(scheduled_delete(format!("sched:{pk}:1:2")).is_err());
    }

    #[test]
    fn test_same_second_create_generates_unique_ids() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("collision");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let at = soshal_common_core::format::now_secs() + 3600;
        db::insert_test_user(&pk);
        let id1 = scheduled_create(pk.clone(), "one".to_string(), at, vec![]).unwrap();
        let id2 = scheduled_create(pk.clone(), "two".to_string(), at, vec![]).unwrap();
        assert_ne!(
            id1, id2,
            "same-second drafts with the same scheduled_at must not collide"
        );
        let arr = parse_arr(&scheduled_list(pk).unwrap());
        assert_eq!(arr.len(), 2, "json: {arr:?}");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn test_delete_other_owners_draft_reports_forbidden() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("forbidden");
        let keys_a = soshal_nostr_core::keys::generate_keys();
        let pk_a = keys_a.public_key().to_hex();
        db::insert_test_user(&pk_a);
        let at = soshal_common_core::format::now_secs() + 3600;
        super::super::signer::signer_unlock(keys_a.secret_key().to_secret_hex()).unwrap();
        let id = scheduled_create(pk_a.clone(), "mine".to_string(), at, vec![]).unwrap();
        // A different identity tries to delete the draft: authorization
        // failure, not a size-cap error.
        let keys_b = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys_b.secret_key().to_secret_hex()).unwrap();
        let res = scheduled_delete(id);
        assert!(res.is_err());
        let err = res.unwrap_err().to_lowercase();
        assert!(
            err.contains("forbidden")
                || err.contains("not authorized")
                || err.contains("identity mismatch"),
            "expected authorization error, got: {err}"
        );
        assert!(
            !err.contains("oversized"),
            "must not mislabel as oversized: {err}"
        );
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_publish_due_persists_real_post_and_clears_draft() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("due");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let due_in = soshal_common_core::format::now_secs() + 2;
        db::insert_test_user(&pk);
        let _id = scheduled_create(
            pk.clone(),
            "due post #soshal".to_string(),
            due_in,
            vec!["hardcoded".to_string()],
        )
        .unwrap();
        assert_eq!(
            parse_arr(&scheduled_list(pk.clone()).unwrap()).len(),
            1,
            "draft must be listed before it comes due"
        );
        tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
        let published = scheduled_publish_due(pk.clone(), 10).await.unwrap();
        assert_eq!(published, 1, "exactly one due draft should publish");
        assert!(
            parse_arr(&scheduled_list(pk.clone()).unwrap()).is_empty(),
            "published draft must leave the draft set"
        );
        let rows = crate::ffi::db::db_query_raw_test(format!(
            "SELECT id, content, sync_status, scheduled_at FROM posts WHERE pubkey = '{pk}'"
        ));
        let found = rows.unwrap();
        assert!(
            found.contains("due post #soshal"),
            "published post missing: {found}"
        );
        super::super::signer::signer_lock().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_blocked_draft_no_longer_starves_valid_due_batch() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let _p = tmp_db("starvation");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        db::insert_test_user(&pk);
        // A blocked word filter makes the blocked draft fail moderation.
        super::super::db::with_db_result(|db| {
            soshal_db_core::repos::settings::SettingsRepo::new(db)
                .set("moderation_word_filters", r#"["blockme"]"#)
        })
        .unwrap();
        let due_now = soshal_common_core::format::now_secs() + 2;
        let _blocked = scheduled_create(
            pk.clone(),
            "post contains blockme flagged word".to_string(),
            due_now,
            vec![],
        )
        .unwrap();
        let _valid = scheduled_create(
            pk.clone(),
            "valid post #soshal".to_string(),
            due_now,
            vec![],
        )
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
        // The blocked draft used to occupy the due-page limit forever; the
        // valid draft must publish on the SAME run while the blocked draft
        // stays scheduled (nudged forward for retry).
        let published = scheduled_publish_due(pk.clone(), 10).await.unwrap();
        assert_eq!(published, 1, "only the valid draft publishes");
        let rows = crate::ffi::db::db_query_raw_test(format!(
            "SELECT content, sync_status, scheduled_at FROM posts WHERE pubkey = '{pk}'"
        ))
        .unwrap();
        assert!(
            rows.contains("valid post #soshal"),
            "valid post not published: {rows}"
        );
        assert!(
            !rows.contains("blocked word"),
            "blocked draft must not be published as a post: {rows}"
        );
        // Blocked draft still listed (user can edit/delete it) and was nudged
        // forward out of the immediate due window.
        let arr = parse_arr(&scheduled_list(pk.clone()).unwrap());
        assert_eq!(arr.len(), 1, "blocked draft must stay scheduled: {arr:?}");
        let nudged = arr[0]["scheduled_at"].as_i64().unwrap();
        let now = soshal_common_core::format::now_secs();
        assert!(
            nudged >= now + SCHEDULE_BLOCKED_RETRY_SECS - 1,
            "blocked draft should be nudged ~{SCHEDULE_BLOCKED_RETRY_SECS}s out, got {nudged} vs now {now}"
        );
        super::super::signer::signer_lock().unwrap();
    }
}
