#[cfg(test)]
mod ffi_aux_modules_tests {
    use soshal_flutter_bridge::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn init_db(name: &str) -> String {
        let path = format!(
            "{}/soshal_aux_{}_{name}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db::db_init(path.clone()).is_ok());
        path
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn bookmarks_ffi_save_list_delete_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("bookmarks");
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
        cleanup(&path);
    }

    #[test]
    fn bookmarks_ffi_resolve_post_unknown_id_empty() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("bookmarks_resolve");
        let got = bookmarks::bookmarks_resolve_post("aux_unknown_post".to_string()).unwrap();
        assert!(got.is_empty());
        cleanup(&path);
    }

    #[test]
    fn headless_ffi_background_sync_migrates_db() {
        let path = format!(
            "{}/soshal_aux_headless_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert_eq!(headless::background_sync_task(path.clone()).unwrap(), 0);
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
        cleanup(&path);
    }

    #[test]
    fn headless_ffi_empty_path_error() {
        let e = headless::background_sync_task(String::new()).unwrap_err();
        assert_eq!(e, "Database path cannot be empty");
    }

    #[tokio::test]
    async fn music_ffi_publish_rejects_bad_audio_url() {
        let e = music::music_publish("not-a-url".to_string(), None, None, Vec::new(), None)
            .await
            .unwrap_err();
        assert_eq!(e, "audio_url must be a valid https media URL");
        let e = music::music_publish(
            "ftp://example.com/track.mp3".to_string(),
            None,
            None,
            Vec::new(),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(e, "audio_url must be a valid https media URL");
    }

    #[tokio::test]
    async fn music_ffi_share_to_feed_message_validation() {
        let e = music::music_share_to_feed(
            "track_1".to_string(),
            "pubkey_1".to_string(),
            "   ".to_string(),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "message must be 1-64000 chars");
        let e = music::music_share_to_feed(
            "track_1".to_string(),
            "pubkey_1".to_string(),
            "x".repeat(64001),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(e, "message must be 1-64000 chars");
    }

    #[test]
    fn push_ffi_register_token_persists() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("push_token");
        let session_path =
            std::path::Path::new(&std::env::temp_dir().to_string_lossy().to_string())
                .join("session.json");
        let _ = std::fs::remove_file(&session_path);
        assert!(session::session_load(path.clone()).is_ok());
        assert!(session::session_add_account(
            "aux_push_pk".to_string(),
            "npub1auxpush".to_string(),
            "[]".to_string()
        )
        .unwrap());
        assert!(push::push_register_token("aux_device_token".to_string()).unwrap());
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&session_path).unwrap()).unwrap();
        assert_eq!(saved["accounts"][0]["push_token"], "aux_device_token");
        assert!(push::push_register_token(String::new()).unwrap());
        let cleared: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&session_path).unwrap()).unwrap();
        assert!(cleared["accounts"][0]["push_token"].is_null());
        let _ = std::fs::remove_file(&session_path);
        cleanup(&path);
    }

    #[test]
    fn relations_ffi_send_friend_request_signer_locked() {
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
        let got = social::social_friend_suggestions().unwrap();
        assert!(got.is_empty());
    }
}
