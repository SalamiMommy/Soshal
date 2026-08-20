//! FFI gap tests: social (friend suggestions), music (publish/fetch error
//! paths), headless background sync, bookmarks CRUD + post resolution.

#[path = "common/mod.rs"]
mod test_util;

#[cfg(test)]
mod bridge_gap_tests {
    use soshal_flutter_bridge::*;
    fn gen_keys() -> (String, String) {
        let keys = soshal_nostr_core::keys::generate_keys();
        (
            keys.public_key().to_hex(),
            keys.secret_key().to_secret_hex(),
        )
    }
    #[test]
    fn friend_suggestions_empty_and_from_graph() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("bridge_gap", "social");
        signer::signer_lock().unwrap();
        assert_eq!(
            social::social_friend_suggestions().unwrap(),
            Vec::<String>::new()
        );
        let (me, secret) = gen_keys();
        let (friend, _) = gen_keys();
        let (mutual, _) = gen_keys();
        signer::signer_unlock(secret).unwrap();
        let insert = |pk: &str, contacts: &str| {
            db::db_execute_params(
                "INSERT INTO users (pubkey, npub, contact_pubkeys, relay_list) VALUES (?1, '', ?2, '[]')",
                &[pk.to_string(), contacts.to_string()],
            )
            .unwrap();
        };
        insert(&me, &format!("[\"{friend}\"]"));
        insert(&friend, &format!("[\"{me}\",\"{mutual}\"]"));
        insert(&mutual, &format!("[\"{me}\"]"));
        let suggestions = social::social_friend_suggestions().unwrap();
        assert!(suggestions.contains(&mutual), "{suggestions:?}");
        let cached = social::social_friend_suggestions().unwrap();
        assert_eq!(suggestions, cached, "60s cache hit");
        let _ = db;
    }
    #[test]
    fn music_publish_and_share_error_paths() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("bridge_gap", "music");
        let (pk, secret) = gen_keys();
        signer::signer_lock().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let err = music::music_publish(
                "ftp://insecure.example.com/a.mp3".into(),
                None,
                None,
                vec![],
                None,
            )
            .await
            .unwrap_err();
            assert!(
                err.contains("media") || err.contains("url") || err.contains("URL"),
                "{err}"
            );
            signer::signer_unlock(secret).unwrap();
            let no_client = music::music_publish(
                "https://example.com/a.mp3".into(),
                Some("t".into()),
                None,
                vec!["tag".into()],
                None,
            )
            .await
            .unwrap_err();
            assert!(
                no_client.contains("relay client not initialized"),
                "{no_client}"
            );
            let empty_msg =
                music::music_share_to_feed("id".into(), pk.clone(), "   ".into(), vec![])
                    .await
                    .unwrap_err();
            assert!(empty_msg.contains("1-64000"), "{empty_msg}");
            let no_client =
                music::music_share_to_feed("id".into(), pk, "hi".into(), vec!["a".into()])
                    .await
                    .unwrap_err();
            assert!(
                no_client.contains("relay client not initialized"),
                "{no_client}"
            );
            let no_client = music::music_comment(31022, "x".repeat(64), "d".into(), "c".into())
                .await
                .unwrap_err();
            assert!(
                no_client.contains("relay client not initialized"),
                "{no_client}"
            );
            let no_client = music::music_fetch(10, None).await.unwrap_err();
            assert!(
                no_client.contains("relay client not initialized"),
                "{no_client}"
            );
            let no_client = music::music_comments(31022, "x".repeat(64), "d".into())
                .await
                .unwrap_err();
            assert!(
                no_client.contains("relay client not initialized"),
                "{no_client}"
            );
            let _ = db;
        });
    }
    #[test]
    fn background_sync_task_guards() {
        let _g = crate::test_util::lock();
        let err =
            soshal_db_core::block_on(headless::background_sync_task(String::new())).unwrap_err();
        assert!(err.contains("empty"), "{err}");
        let missing = "/nonexistent/soshal/sync.db".to_string();
        let err = soshal_db_core::block_on(headless::background_sync_task(missing)).unwrap_err();
        assert!(err.contains("Failed to open DB"), "{err}");
        let db = crate::test_util::init_db("bridge_gap", "headless");
        assert_eq!(
            soshal_db_core::block_on(headless::background_sync_task(db)).unwrap(),
            0
        );
        let _ = db;
    }
    #[test]
    fn bookmarks_crud_and_resolve() {
        let _g = crate::test_util::lock();
        let db = crate::test_util::init_db("bridge_gap", "bookmarks");
        let (pk, _) = gen_keys();
        let id = bookmarks::bookmarks_save(pk.clone(), "evt1".into()).unwrap();
        assert_eq!(id, "bm:evt1");
        let id2 = bookmarks::bookmarks_save(pk.clone(), "evt2".into()).unwrap();
        assert_eq!(id2, "bm:evt2");
        let list = bookmarks::bookmarks_list(pk.clone(), 10, 0).unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        let paged = bookmarks::bookmarks_list(pk.clone(), 1, 1).unwrap();
        let pv: serde_json::Value = serde_json::from_str(&paged).unwrap();
        assert_eq!(pv.as_array().unwrap().len(), 1);
        assert!(bookmarks::bookmarks_delete("bm:evt1".into()).unwrap());
        assert!(bookmarks::bookmarks_list(pk, 10, 0)
            .unwrap()
            .contains("evt2"));
        let empty = bookmarks::bookmarks_resolve_post("nope".into()).unwrap();
        assert_eq!(empty, "");
        let (other, _) = gen_keys();
        db::db_execute_params(
            "INSERT INTO users (pubkey, npub, relay_list) VALUES (?1, '', '[]')",
            &[other.clone()],
        )
        .unwrap();
        db::db_execute_params(
            "INSERT INTO posts (id, pubkey, content, kind, created_at) VALUES ('evt2', ?1, 'hello', 1, 1700000000)",
            &[other],
        )
        .unwrap();
        let resolved = bookmarks::bookmarks_resolve_post("evt2".into()).unwrap();
        assert!(resolved.contains("\"content\":\"hello\""), "{resolved}");
        let map = bookmarks::bookmarks_resolve_posts(r#"["evt2","missing"]"#.into()).unwrap();
        let mv: serde_json::Value = serde_json::from_str(&map).unwrap();
        assert!(mv.get("evt2").is_some());
        assert!(mv.get("missing").is_none());
        let bad = bookmarks::bookmarks_resolve_posts("not-json".into()).unwrap_err();
        assert!(bad.contains("invalid ids JSON"), "{bad}");
        let _ = db;
    }
}
