//! FFI gap tests (round 2): search index, ephemeral media, notifications,
//! outbox queue, events reminders/interest scoring.

#[cfg(test)]
mod ffi_more_gap_tests {
    use soshal_flutter_bridge::*;
    fn gen_keys() -> (String, String) {
        let keys = soshal_nostr_core::keys::generate_keys();
        (
            keys.public_key().to_hex(),
            keys.secret_key().to_secret_hex(),
        )
    }
    #[test]
    fn search_index_query_trending_and_remove() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("more_gap", "search");
        let (pk, _) = gen_keys();
        let rows = format!(
            r#"[{{"id":"p1","pubkey":"{pk}","content":"hello nostr world","kind":1,"created_at":1700000000,"tags_json":"[]"}}]"#
        );
        assert!(search::search_index_posts(rows).unwrap());
        let pk2 = "c".repeat(64);
        db::db_execute_params(
            "INSERT INTO users (pubkey, npub, relay_list) VALUES (?1, '', '[]'), (?2, '', '[]')",
            &[pk.clone(), pk2.clone()],
        )
        .unwrap();
        db::db_execute_params(
            "INSERT INTO posts (id, pubkey, content, kind, created_at) VALUES ('p1', ?1, 'hello nostr world', 1, 1700000000), ('p2', ?2, 'Alice Smith bio', 0, 1700000001)",
            &[pk.clone(), pk2],
        )
        .unwrap();
        let posts = search::search_posts("nostr".into(), 10, "public".into()).unwrap();
        assert!(posts.contains("hello nostr world"), "{posts}");
        let none = search::search_posts("zzz".into(), 10, "public".into()).unwrap();
        assert_eq!(none, "[]");
        assert!(search::search_index_profile(pk, "Alice Smith".into(), "bio".into()).unwrap());
        let profiles = search::search_profiles("alice".into(), 10).unwrap();
        assert!(profiles.contains("Alice Smith"), "{profiles}");
        let global = search::search_global("nostr".into(), 10, "public".into()).unwrap();
        assert!(global.contains("hello nostr world"), "{global}");
        let mentions = search::search_mentions("ali".into(), 10).unwrap();
        assert!(mentions.contains("Alice Smith"), "{mentions}");
        let tags = search::search_hashtags("nostr".into(), 10).unwrap();
        assert_eq!(tags, Vec::<String>::new(), "no hashtag rows yet");
        let trending = search::search_trending_hashtags(10).unwrap();
        assert_eq!(trending, Vec::<String>::new());
        let empty_tags = search::search_hashtags("   ".into(), 10).unwrap();
        assert_eq!(empty_tags, Vec::<String>::new());
        assert!(search::search_remove_indexed("p1".into()).unwrap());
        let _ = db;
    }
    #[test]
    fn ephemeral_media_lifecycle() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("more_gap", "ephemeral");
        let (sender, _) = gen_keys();
        let (recipient, _) = gen_keys();
        let err = ephemeral::ephemeral_save(
            "m1".into(),
            "c1".into(),
            "dm".into(),
            "https://example.com/v.mp4".into(),
            "video".into(),
            sender.clone(),
            recipient.clone(),
            0,
            1700000000,
        )
        .unwrap_err();
        assert!(err.contains("max_views"), "{err}");
        let id = ephemeral::ephemeral_save(
            "m1".into(),
            "c1".into(),
            "dm".into(),
            "https://example.com/v.mp4".into(),
            "video".into(),
            sender,
            recipient.clone(),
            3,
            1893456000,
        )
        .unwrap();
        let got = ephemeral::ephemeral_get(id.clone()).unwrap();
        assert!(got.contains("\"state\":\"pending\""), "{got}");
        let pending = ephemeral::ephemeral_list_pending(recipient).unwrap();
        assert!(pending.contains(&id), "{pending}");
        let viewed = ephemeral::ephemeral_view(id.clone()).unwrap();
        assert!(viewed.contains("\"current_views\":1"), "{viewed}");
        let missing = ephemeral::ephemeral_get("nope".into()).unwrap_err();
        assert!(missing.contains("not found"), "{missing}");
        assert!(ephemeral::ephemeral_delete(id.clone()).unwrap());
        let gone = ephemeral::ephemeral_get(id).unwrap_err();
        assert!(gone.contains("not found"), "{gone}");
        let cleaned = ephemeral::ephemeral_clean_expired().unwrap();
        assert!(cleaned.is_empty());
        let _ = db;
    }
    #[test]
    fn notifications_crud_and_push_scoping() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("more_gap", "notify");
        let (me, me_sec) = gen_keys();
        signer::signer_unlock(me_sec).unwrap();
        let (other, _) = gen_keys();
        crate::test_util::insert_user(&me);
        crate::test_util::insert_user(&other);
        assert_eq!(
            notifications::notifications_get_unread_count(me.clone()).unwrap(),
            0
        );
        let fetch = notifications::notifications_fetch(me.clone(), 10, 0).unwrap();
        assert_eq!(fetch, "[]");
        let unread = notifications::notifications_fetch_unread(me.clone(), 10).unwrap();
        assert_eq!(unread, "[]");
        let insert = |id: &str, typ: &str, read: i64| {
            db::db_execute_params(
                "INSERT INTO notifications (id, pubkey, type, event_id, from_pubkey, content, created_at, is_read) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 'hi', 1700000000, ?6)",
                &[
                    id.to_string(),
                    me.clone(),
                    typ.to_string(),
                    format!("e{id}"),
                    other.clone(),
                    read.to_string(),
                ],
            )
            .unwrap();
        };
        insert("n1", "mention", 0);
        insert("n2", "like", 0);
        insert("n3", "follow", 0);
        assert_eq!(
            notifications::notifications_get_unread_count(me.clone()).unwrap(),
            3
        );
        let all = notifications::notifications_fetch(me.clone(), 10, 0).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&all)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            3
        );
        let unread = notifications::notifications_fetch_unread(me.clone(), 10).unwrap();
        let ua: serde_json::Value = serde_json::from_str(&unread).unwrap();
        assert_eq!(ua.as_array().unwrap().len(), 3);
        let by_type =
            notifications::notifications_fetch_by_type(me.clone(), "mention".into(), 10).unwrap();
        assert!(by_type.contains("n1"), "{by_type}");
        let mentions =
            notifications::notifications_fetch_by_type(me.clone(), "mention".to_string(), 10)
                .unwrap();
        assert!(mentions.contains("n1"), "{mentions}");
        let reactions =
            notifications::notifications_fetch_by_type(me.clone(), "like".to_string(), 10).unwrap();
        assert!(reactions.contains("n2"), "{reactions}");
        let replies =
            notifications::notifications_fetch_by_type(me.clone(), "reply".to_string(), 10)
                .unwrap();
        assert_eq!(replies, "[]");
        let messages =
            notifications::notifications_fetch_by_type(me.clone(), "message".to_string(), 10)
                .unwrap();
        assert_eq!(messages, "[]");
        let follows =
            notifications::notifications_fetch_by_type(me.clone(), "follow".to_string(), 10)
                .unwrap();
        assert!(follows.contains("n3"), "{follows}");
        assert!(notifications::notifications_mark_read("n1".into()).unwrap());
        assert_eq!(
            notifications::notifications_get_unread_count(me.clone()).unwrap(),
            2
        );
        assert!(notifications::notifications_mark_all_read(me.clone()).unwrap());
        assert_eq!(
            notifications::notifications_get_unread_count(me.clone()).unwrap(),
            0
        );
        assert!(notifications::notifications_delete("n2".into()).unwrap());
        let after = notifications::notifications_fetch(me.clone(), 10, 0).unwrap();
        assert!(!after.contains("n2"), "{after}");
        let _ = session::session_clear();
        let unloaded =
            notifications::notifications_register_push(me.clone(), "tok".into()).unwrap_err();
        assert!(unloaded.contains("Session not loaded"), "{unloaded}");
        let _ = signer::signer_lock();
        let _ = db;
    }
    #[test]
    fn outbox_enqueue_and_summary() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("more_gap", "outbox");
        let id =
            sync::sync_enqueue_outbox("post".into(), r#"{"content":"hi"}"#.into(), None).unwrap();
        let id2 = sync::sync_enqueue_outbox(
            "media".into(),
            r#"{"content":"v"}"#.into(),
            Some("/tmp/x.mp4".into()),
        )
        .unwrap();
        let summary = sync::sync_get_outbox_summary().unwrap();
        let v: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(v["pending_count"], 2, "{summary}");
        assert_ne!(id, id2);
        assert!(!sync::sync_running().unwrap());
        let _ = sync::sync_running().unwrap();
        let _ = db;
    }
    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn events_reminders_and_interest_scoring() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("more_gap", "events");
        let (_, sec) = gen_keys();
        signer::signer_unlock(sec).unwrap();
        let err =
            events::events_reminder_upsert(String::new(), "e1".into(), "R".into(), 1700000000, -1)
                .unwrap_err();
        assert!(err.contains("minutes_before"), "{err}");
        let rid = events::events_reminder_upsert(
            String::new(),
            "e1".into(),
            "Launch".into(),
            1700000000,
            15,
        )
        .unwrap();
        assert!(rid.starts_with("rem_"), "{rid}");
        let again = events::events_reminder_upsert(
            rid.clone(),
            "e1".into(),
            "Launch 2".into(),
            1700000000,
            15,
        )
        .unwrap();
        assert_eq!(again, rid, "upsert same id");
        let list = events::events_reminders_list().unwrap();
        assert!(list.contains("Launch 2"), "{list}");
        assert!(events::events_reminder_delete(rid).unwrap());
        let score =
            events::events_interest_score(r#"["music"]"#.into(), r#"["music","art"]"#.into())
                .unwrap();
        assert!(score.starts_with('{'), "{score}");
        let scored = events::events_score_events(
            r#"[{"id":"ev1","title":"Jazz night","description":"live music"}]"#.into(),
            r#"["music"]"#.into(),
        )
        .unwrap();
        let sv: serde_json::Value = serde_json::from_str(&scored).unwrap();
        assert!(sv.get("ev1").is_some(), "{scored}");
        let bad = events::events_score_events("junk".into(), "[]".into()).unwrap_err();
        assert!(bad.contains("invalid events JSON"), "{bad}");
        let nearby = events::events_fetch_nearby(37.0, -122.0, 5.0, 10, "public".into())
            .await
            .unwrap();
        assert_eq!(nearby, "[]");
        let user_events = events::events_fetch_user_events("a".repeat(64), 10)
            .await
            .unwrap();
        assert_eq!(user_events, "[]");
        let single = events::events_get_event("nope".into()).unwrap_err();
        assert!(single.contains("not found"), "{single}");
        assert_eq!(
            events::events_get_attendees("nope".into()).unwrap(),
            Vec::<String>::new()
        );
        let _ = signer::signer_lock();
        let _ = db;
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread")]
    async fn feed_reply_reaction_delete_lifecycle() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("more_gap", "feed_lifecycle");
        let keys = soshal_nostr_core::keys::generate_keys();
        let author_pk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        // Insert author's post into DB
        let post_id =
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string();
        crate::test_util::insert_user(&author_pk);
        db::db_execute_params(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES (?1, ?2, 'root post', 1, 1000, '[]', 'synced', 0)",
            &[post_id.clone(), author_pk.clone()],
        )
        .unwrap();

        // feed_publish_reply: should include NIP-10 'p' tag for author_pk
        let reply_json =
            feed::feed_publish_reply("my reply".into(), post_id.clone(), post_id.clone())
                .await
                .unwrap();
        let reply_val: serde_json::Value = serde_json::from_str(&reply_json).unwrap();
        let tags = reply_val["tags"].as_array().unwrap();
        let has_p_tag = tags.iter().any(|t| {
            t.get(0).and_then(|v| v.as_str()) == Some("p")
                && t.get(1).and_then(|v| v.as_str()) == Some(&author_pk)
        });
        assert!(
            has_p_tag,
            "reply must contain NIP-10 p-tag for root author: {reply_json}"
        );

        // feed_create_reaction: should include NIP-25 'p' tag for author_pk
        let reaction_json = feed::feed_create_reaction(post_id.clone(), "+".into())
            .await
            .unwrap();
        let react_val: serde_json::Value = serde_json::from_str(&reaction_json).unwrap();
        let react_tags = react_val["tags"].as_array().unwrap();
        let has_react_p = react_tags.iter().any(|t| {
            t.get(0).and_then(|v| v.as_str()) == Some("p")
                && t.get(1).and_then(|v| v.as_str()) == Some(&author_pk)
        });
        assert!(
            has_react_p,
            "reaction must contain NIP-25 p-tag for reacted author: {reaction_json}"
        );

        // feed_delete_post: rejects invalid hex
        let bad_hex = feed::feed_delete_post("short".into()).await.unwrap_err();
        assert!(bad_hex.contains("64-character hex"));

        // Another signer cannot delete author's post
        let eve_keys = soshal_nostr_core::keys::generate_keys();
        signer::signer_unlock(eve_keys.secret_key().to_secret_hex()).unwrap();
        let err = feed::feed_delete_post(post_id.clone()).await.unwrap_err();
        assert!(err.contains("cannot delete post authored by another user"));

        // Author deletes their own post -> soft-deleted locally in SQLite immediately
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let del_json = feed::feed_delete_post(post_id.clone()).await.unwrap();
        assert!(del_json.contains("deleted by user"));
        let raw = db::db_query_raw(format!(
            "SELECT is_deleted FROM posts WHERE id = '{post_id}'"
        ))
        .unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            rows[0]["is_deleted"], 1,
            "post must be marked deleted in local SQLite database"
        );

        signer::signer_lock().unwrap();
        crate::test_util::cleanup(&path);
    }

    #[test]
    fn identity_follow_self_and_invalid_pubkey() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("more_gap", "identity_self");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        // Invalid pubkey format rejected
        assert!(identity::identity_follow_user("not_hex".into())
            .unwrap_err()
            .contains("64-character hex"));
        assert!(identity::identity_unfollow_user("not_hex".into())
            .unwrap_err()
            .contains("64-character hex"));

        // Self-follow rejected
        assert!(identity::identity_follow_user(pk)
            .unwrap_err()
            .contains("cannot follow yourself"));

        signer::signer_lock().unwrap();
        crate::test_util::cleanup(&path);
    }
}
