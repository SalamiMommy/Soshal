#[path = "common/mod.rs"]
mod test_util;

#[cfg(test)]
mod ffi_aux_modules_tests {
    use soshal_flutter_bridge::*;
    #[test]
    fn bookmarks_ffi_save_list_delete_roundtrip() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("aux", "bookmarks");
        let pubkey = "aux_bookmark_user".to_string();
        let event_id = "aux_event_001".to_string();
        let id = bookmarks::bookmarks_save(pubkey.clone(), event_id.clone()).unwrap();
        assert_eq!(id, format!("bm:{event_id}"));
        let list = bookmarks::bookmarks_list(pubkey.clone(), 10, 0).unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["event_id"], event_id);
        assert!(bookmarks::bookmarks_delete(id).unwrap());
        let list = bookmarks::bookmarks_list(pubkey, 10, 0).unwrap();
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert!(v.as_array().unwrap().is_empty());
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn bookmarks_ffi_resolve_post_unknown_id_empty() {
        let _g = crate::test_util::lock();
        let path = crate::test_util::init_db("aux", "bookmarks_resolve");
        let got = bookmarks::bookmarks_resolve_post("aux_unknown_post".to_string()).unwrap();
        assert!(got.is_empty());
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn headless_ffi_background_sync_migrates_db() {
        let _g = crate::test_util::lock();
        let path = format!(
            "{}/soshal_aux_headless_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert_eq!(
            soshal_db_core::block_on(headless::background_sync_task(path.clone())).unwrap(),
            0
        );
        let db = soshal_db_core::Database::open(&path).unwrap();
        let conn = db.conn().unwrap();
        let max: i64 = soshal_db_core::block_on(async {
            let mut rows = conn
                .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
                .await
                .unwrap();
            rows.next().await.unwrap().unwrap().get(0).unwrap()
        });
        assert_eq!(max, soshal_db_core::schema::SCHEMA_VERSION);
        crate::test_util::cleanup(&path);
    }
    #[test]
    fn headless_ffi_empty_path_error() {
        let e =
            soshal_db_core::block_on(headless::background_sync_task(String::new())).unwrap_err();
        assert_eq!(e, "Database path cannot be empty");
    }
    #[tokio::test]
    async fn music_ffi_publish_rejects_bad_audio_url() {
        let e = music::music_publish("not-a-url".to_string(), None, None, Vec::new(), None)
            .await
            .unwrap_err();
        assert!(
            e.contains("media") || e.contains("url") || e.contains("URL"),
            "{e}"
        );
        let e = music::music_publish(
            "ftp://example.com/track.mp3".to_string(),
            None,
            None,
            Vec::new(),
            None,
        )
        .await
        .unwrap_err();
        assert!(
            e.contains("media") || e.contains("url") || e.contains("URL"),
            "{e}"
        );
    }
    #[tokio::test]
    async fn music_ffi_share_to_feed_message_validation() {
        let e = music::music_share_to_feed(
            "track_1".to_string(),
            "pubkey_1".to_string(),
            "d_tag_1".to_string(),
            "   ".to_string(),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "message must be 1-64000 chars");
        let e = music::music_share_to_feed(
            "track_1".to_string(),
            "pubkey_1".to_string(),
            "d_tag_1".to_string(),
            "x".repeat(64001),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "message must be 1-64000 chars");
    }
    #[test]
    fn relations_ffi_send_friend_request_signer_locked() {
        // Lock: asserts the signer is locked, which must not race sibling
        // tests that unlock the signer.
        let _g = crate::test_util::lock();
        let e = relations::relations_send_friend_request("aux_pubkey".to_string()).unwrap_err();
        assert_eq!(e, "signer locked");
    }
    #[tokio::test]
    async fn protocol_ffi_traversal_path_rejected() {
        let r = protocol_handler::protocol_handle_request(
            "app".to_string(),
            "media".to_string(),
            "local/../../etc/passwd".to_string(),
        )
        .await;
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert!(!e.is_empty());
    }
    #[tokio::test]
    async fn protocol_ffi_scheme_and_host_contracts() {
        let e = protocol_handler::protocol_handle_request(
            "file".to_string(),
            "x".to_string(),
            "/y".to_string(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "Unsupported scheme: file");
        let e = protocol_handler::protocol_handle_request(
            "app".to_string(),
            "unknown".to_string(),
            "/y".to_string(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "Unknown app:// host: unknown");
    }
    #[test]
    fn social_ffi_friend_suggestions_no_signer_empty() {
        // Lock: signer state is process-global across parallel tests.
        let _g = crate::test_util::lock();
        let got = social::social_friend_suggestions().unwrap();
        assert!(got.is_empty());
    }
    #[test]
    fn session_ffi_add_list_switch_active() {
        let _g = crate::test_util::lock();
        let dir = format!(
            "{}/soshal_aux_session_{}_addlist",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = format!("{dir}/app.db");
        assert!(db::db_init(db_path.clone()).is_ok());
        session::session_load(db_path).unwrap();
        session::session_add_account(
            "spk1".to_string(),
            "npub1spk1".to_string(),
            "[\"wss://relay.a\"]".to_string(),
        )
        .unwrap();
        session::session_add_account(
            "spk2".to_string(),
            "npub1spk2".to_string(),
            "[]".to_string(),
        )
        .unwrap();
        let list = session::session_list_accounts().unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&list)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2);
        assert!(session::session_switch_account("spk2".to_string()).unwrap());
        let active = session::session_get_active().unwrap();
        assert!(active.contains("\"pubkey\":\"spk2\""));
        std::fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn session_ffi_save_load_roundtrip() {
        let _g = crate::test_util::lock();
        let dir = format!(
            "{}/soshal_aux_session_{}_roundtrip",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = format!("{dir}/app.db");
        assert!(db::db_init(db_path.clone()).is_ok());
        let data = r#"{"active_pubkey":"spk1","accounts":[{"pubkey":"spk1","npub":"npub1spk1","last_used":1,"relay_list":[]}]}"#;
        assert!(session::session_save(db_path.clone(), data.to_string()).unwrap());
        let loaded = session::session_load(db_path).unwrap();
        assert!(loaded.contains("\"active_pubkey\":\"spk1\""));
        let active = session::session_get_active().unwrap();
        assert!(active.contains("\"pubkey\":\"spk1\""));
        std::fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn session_ffi_push_token_register_and_clear() {
        let _g = crate::test_util::lock();
        let dir = format!(
            "{}/soshal_aux_session_{}_push",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = format!("{dir}/app.db");
        assert!(db::db_init(db_path.clone()).is_ok());
        session::session_load(db_path.clone()).unwrap();
        session::session_add_account(
            "spk1".to_string(),
            "npub1spk1".to_string(),
            "[]".to_string(),
        )
        .unwrap();
        assert!(session::session_register_push_token("aux_tok".to_string()).unwrap());
        let reloaded = session::session_load(db_path.clone()).unwrap();
        assert!(reloaded.contains("aux_tok"));
        assert!(session::session_register_push_token(String::new()).unwrap());
        let reloaded = session::session_load(db_path).unwrap();
        assert!(!reloaded.contains("aux_tok"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
