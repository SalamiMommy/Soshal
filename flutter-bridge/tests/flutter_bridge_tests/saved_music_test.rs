#[cfg(test)]
mod saved_music_tests {
    use serde_json::json;
    use soshal_flutter_bridge::*;

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn test_saved_minis_and_tracks_workflow() {
        let _t = soshal_test_util::test_lock();
        let path = soshal_test_util::tmp_path("saved", "saved_flow.db")
            .to_string_lossy()
            .to_string();
        cleanup(&path);
        db::db_init(path.clone()).unwrap();
        db::db_execute_params("INSERT INTO users (pubkey, npub) VALUES ('pk','npk');", &[])
            .unwrap();
        db::db_execute_params(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) VALUES ('m1','pk','save me',31020,100,'[[\"url\",\"https://mini.example/a\"]]','pending',0);",
            &[],
        )
        .unwrap();
        let empty: Vec<serde_json::Value> =
            serde_json::from_str(&minis::minis_saved().unwrap()).unwrap();
        assert!(empty.is_empty());
        assert!(!minis::minis_save("m1".to_string()).unwrap());
        let saved: Vec<serde_json::Value> =
            serde_json::from_str(&minis::minis_saved().unwrap()).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["videoUrl"], "https://mini.example/a");
        assert_eq!(saved[0]["hostReady"], false);
        assert!(minis::minis_unsave("m1".to_string()).unwrap());
        let after: Vec<serde_json::Value> =
            serde_json::from_str(&minis::minis_saved().unwrap()).unwrap();
        assert!(after.is_empty());
        let t = json!({"id":"t1","pubkey":"pk","audioUrl":"https://example.com/t1.mp3","blobHash":"","mediaSize":123,"title":"Song","thumbnail":"","hashtags":["a"],"d":"soshal_music_1","audience":"public","createdAt":100});
        assert!(!music::music_save(t.to_string()).unwrap());
        let tracks: Vec<serde_json::Value> =
            serde_json::from_str(&music::music_saved().unwrap()).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0]["title"], "Song");
        assert_eq!(tracks[0]["hostReady"], false);
        assert!(music::music_unsave("t1".to_string()).unwrap());
        let tracks_after: Vec<serde_json::Value> =
            serde_json::from_str(&music::music_saved().unwrap()).unwrap();
        assert!(tracks_after.is_empty());
        db::db_close().unwrap();
        cleanup(&path);
    }

    #[test]
    fn test_playlist_workflow() {
        let _t = soshal_test_util::test_lock();
        let path = soshal_test_util::tmp_path("saved", "playlist_flow.db")
            .to_string_lossy()
            .to_string();
        cleanup(&path);
        db::db_init(path.clone()).unwrap();
        let p1 = music::music_playlist_create("Likes".to_string(), false).unwrap();
        assert!(!p1.is_empty());
        assert!(music::music_playlist_create(" ".to_string(), true)
            .unwrap_err()
            .contains("title"));
        let t = json!({"id":"t1","pubkey":"pk","audioUrl":"https://example.com/t1.mp3","blobHash":"","mediaSize":123,"title":"Song","thumbnail":"","hashtags":["a"],"d":"soshal_music_1","audience":"public","createdAt":100});
        assert!(music::music_playlist_add_track(p1.clone(), t.to_string()).unwrap());
        assert!(music::music_playlist_add_track(p1.clone(), t.to_string()).unwrap());
        let list: Vec<serde_json::Value> =
            serde_json::from_str(&music::music_playlist_list().unwrap()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["trackCount"], 1);
        let tracks: Vec<serde_json::Value> =
            serde_json::from_str(&music::music_playlist_tracks(p1.clone()).unwrap()).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0]["id"], "t1");
        assert!(music::music_playlist_rename(p1.clone(), "Faves".to_string()).unwrap());
        let renamed: Vec<serde_json::Value> =
            serde_json::from_str(&music::music_playlist_list().unwrap()).unwrap();
        assert_eq!(renamed[0]["title"], "Faves");
        assert!(music::music_playlist_delete(p1.clone()).unwrap());
        let after: Vec<serde_json::Value> =
            serde_json::from_str(&music::music_playlist_list().unwrap()).unwrap();
        assert!(after.is_empty());
        db::db_close().unwrap();
        cleanup(&path);
    }
}
