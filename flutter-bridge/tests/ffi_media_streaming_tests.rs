#[path = "common/mod.rs"]
mod test_util;

#[cfg(test)]
mod ffi_media_streaming_tests {
    use soshal_flutter_bridge::*;
    const PNG_1X1_RGBA: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    fn temp_path(tag: &str) -> String {
        soshal_test_util::tmp_path("media_stream", tag)
            .to_string_lossy()
            .to_string()
    }
    fn db_path(tag: &str) -> String {
        soshal_test_util::tmp_path("media_stream", &format!("{tag}.db"))
            .to_string_lossy()
            .to_string()
    }
    fn remove_db(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }
    fn unique_bytes() -> Vec<u8> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seed = (nanos % u64::MAX as u128) as u64;
        (0..64 * 1024)
            .map(|i| ((seed as usize + i) % 253) as u8)
            .collect()
    }
    fn unlock_test_signer() -> String {
        let secret = "01".repeat(32);
        signer::signer_unlock(secret).unwrap();
        signer::signer_pubkey().unwrap()
    }
    #[test]
    fn media_decode_png_rgba() {
        let path = temp_path("decode");
        std::fs::write(&path, PNG_1X1_RGBA).unwrap();
        let dto = media::media_decode_image_rgba(path.clone(), None, None).unwrap();
        assert_eq!(dto.width, 1);
        assert_eq!(dto.height, 1);
        assert_eq!(dto.pixels.len(), 4);
        let missing = temp_path("decode_missing");
        assert!(media::media_decode_image_rgba(missing, None, None).is_err());
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn media_mime_mapping() {
        assert_eq!(
            media::media_get_mime_type("photo.PNG".to_string()).unwrap(),
            "image/png"
        );
        assert_eq!(
            media::media_get_mime_type("a.jpeg".to_string()).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            media::media_get_mime_type("b.gif".to_string()).unwrap(),
            "image/gif"
        );
        assert_eq!(
            media::media_get_mime_type("c.webp".to_string()).unwrap(),
            "image/webp"
        );
        assert_eq!(
            media::media_get_mime_type("d.mp4".to_string()).unwrap(),
            "video/mp4"
        );
        assert_eq!(
            media::media_get_mime_type("e.webm".to_string()).unwrap(),
            "video/webm"
        );
        assert_eq!(
            media::media_get_mime_type("f.mov".to_string()).unwrap(),
            "video/quicktime"
        );
        assert_eq!(
            media::media_get_mime_type("g.wav".to_string()).unwrap(),
            "audio/wav"
        );
        assert_eq!(
            media::media_get_mime_type("h.mp3".to_string()).unwrap(),
            "audio/mpeg"
        );
        assert_eq!(
            media::media_get_mime_type("i.m4a".to_string()).unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            media::media_get_mime_type("j.xyz".to_string()).unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            media::media_get_mime_type("noext".to_string()).unwrap(),
            "application/octet-stream"
        );
    }
    #[test]
    fn media_cache_path_and_clear() {
        let cache = media::media_get_cache_path().unwrap();
        assert!(!cache.is_empty());
        let dir = temp_path("clear");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(media::media_clear_cache(dir.clone()).is_ok());
        assert!(media::media_clear_cache(dir).is_err());
    }
    #[test]
    fn media_local_server_start_stop() {
        let _g = crate::test_util::lock();
        let port1 = media::media_start_local_server().unwrap();
        assert!(port1 > 0);
        let port2 = media::media_start_local_server().unwrap();
        assert_eq!(port1, port2);
        assert!(media::media_stop_local_server().unwrap());
        assert!(media::media_stop_local_server().unwrap());
    }
    #[test]
    fn media_blob_upload_fetch_roundtrip() {
        let _g = crate::test_util::lock();
        let data = unique_bytes();
        let json = media::media_upload_blob(data.clone()).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();
        let hash = manifest["blob_hash"].as_str().unwrap().to_string();
        let total = manifest["total_size"].as_u64().unwrap();
        assert_eq!(total, data.len() as u64);
        let out = temp_path("blob_out");
        let res = media::media_fetch_blob(hash.clone(), out.clone()).unwrap();
        assert!(res.contains("\"success\":true"));
        assert!(res.contains(&hash));
        assert_eq!(std::fs::read(&out).unwrap(), data);
        let out2 = temp_path("blob_miss");
        assert!(media::media_fetch_blob("ab".repeat(32), out2.clone()).is_err());
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&out2);
    }
    #[tokio::test]
    async fn media_upload_blob_file_roundtrip() {
        let _g = crate::test_util::lock();
        let data = unique_bytes();
        let src = temp_path("blob_src");
        std::fs::write(&src, &data).unwrap();
        let json = media::media_upload_blob_file(src.clone()).await.unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();
        let hash = manifest["blob_hash"].as_str().unwrap().to_string();
        assert_eq!(manifest["total_size"].as_u64().unwrap(), data.len() as u64);
        let out = temp_path("blob_file_out");
        media::media_fetch_blob(hash, out.clone()).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), data);
        assert!(media::media_upload_blob_file(temp_path("no_such_file"))
            .await
            .is_err());
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
    }
    #[tokio::test]
    async fn media_upload_blob_file_rejects_ssrf_urls() {
        let _g = crate::test_util::lock();
        for url in [
            "http://localhost:8080/a.jpg",
            "http://127.0.0.1/a.jpg",
            "http://192.168.1.10/a.jpg",
            "http://foo.localtest.me/a.jpg",
        ] {
            let err = media::media_upload_blob_file(url.to_string())
                .await
                .unwrap_err();
            assert!(
                err.contains("not allowed") || err.contains("internal address"),
                "{url}: {err}"
            );
        }
    }
    #[test]
    fn media_load_local_roundtrip_and_missing() {
        let data = unique_bytes();
        let path = temp_path("local");
        std::fs::write(&path, &data).unwrap();
        assert_eq!(media::media_load_local(path.clone()).unwrap(), data);
        assert!(media::media_load_local(temp_path("missing")).is_err());
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn streaming_fetch_empty_with_fresh_db() {
        let _g = crate::test_util::lock();
        let path = db_path("empty");
        remove_db(&path);
        assert!(db::db_init(path.clone()).is_ok());
        assert_eq!(streaming::streaming_fetch_live(10).unwrap(), "[]");
        assert_eq!(
            streaming::streaming_fetch_followed_live("pk1".to_string()).unwrap(),
            "[]"
        );
        assert_eq!(
            streaming::streaming_fetch_stories("pk1".to_string()).unwrap(),
            "[]"
        );
        assert_eq!(
            streaming::streaming_fetch_followed_stories("pk1".to_string()).unwrap(),
            "[]"
        );
        remove_db(&path);
    }
    #[test]
    fn streaming_story_roundtrip() {
        let _g = crate::test_util::lock();
        let path = db_path("story");
        remove_db(&path);
        assert!(db::db_init(path.clone()).is_ok());
        let pk = unlock_test_signer();
        let signed = streaming::streaming_post_story(
            pk.clone(),
            "hello stories".to_string(),
            "[]".to_string(),
            24,
        )
        .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        let id = ev["id"].as_str().unwrap().to_string();
        assert_eq!(id.len(), 64);
        let stories = streaming::streaming_fetch_stories(pk.clone()).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&stories).unwrap();
        assert_eq!(arr[0]["id"], id);
        assert_eq!(arr[0]["content"], "hello stories");
        assert!(streaming::streaming_mark_story_viewed(id.clone(), pk.clone()).unwrap());
        let react = streaming::streaming_story_react(id.clone(), pk, "❤️".to_string());
        assert!(react.is_ok());
        signer::signer_lock().unwrap();
        remove_db(&path);
    }
    #[test]
    fn streaming_live_roundtrip() {
        let _g = crate::test_util::lock();
        let path = db_path("live");
        remove_db(&path);
        assert!(db::db_init(path.clone()).is_ok());
        let pk = unlock_test_signer();
        let bad = streaming::streaming_start_live(
            pk.clone(),
            String::new(),
            "d".to_string(),
            "https://example.com/s".to_string(),
        );
        assert!(bad.is_err());
        let signed = streaming::streaming_start_live(
            pk.clone(),
            "Test Stream".to_string(),
            "desc".to_string(),
            "https://example.com/s".to_string(),
        )
        .unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        let id = ev["id"].as_str().unwrap().to_string();
        assert_eq!(id.len(), 64);
        assert!(streaming::streaming_end_live(id.clone(), "wrong".to_string()).is_err());
        let live = streaming::streaming_fetch_live(10).unwrap();
        let arr: serde_json::Value = serde_json::from_str(&live).unwrap();
        assert_eq!(arr[0]["id"], id);
        assert_eq!(arr[0]["status"], "live");
        assert!(streaming::streaming_end_live(id.clone(), pk.clone()).unwrap());
        let ended = streaming::streaming_fetch_live(10).unwrap();
        let arr2: serde_json::Value = serde_json::from_str(&ended).unwrap();
        assert_eq!(arr2[0]["id"], id);
        assert_eq!(arr2[0]["status"], "ended");
        signer::signer_lock().unwrap();
        remove_db(&path);
    }
    #[test]
    fn streaming_moq_publish_object() {
        let key = streaming::streaming_moq_publish_object(
            "sid1".to_string(),
            "pk".to_string(),
            7,
            true,
            "0102ff".to_string(),
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&key).unwrap();
        let h = &parsed["header"];
        assert_eq!(h["track_id"], 7);
        assert_eq!(h["track_type"], "VideoKeyframe");
        assert_eq!(h["group_sequence"], 1, "keyframe bumps group seq");
        assert_eq!(h["object_sequence"], 3, "payload 3 bytes");
        assert_eq!(h["payload_size"], 3);
        assert!(h["timestamp_ms"].as_u64().unwrap() > 0);
        let delta = streaming::streaming_moq_publish_object(
            "sid1".to_string(),
            "pk".to_string(),
            7,
            false,
            "ff".to_string(),
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&delta).unwrap();
        let h = &parsed["header"];
        assert_eq!(h["track_type"], "VideoDelta");
        assert_eq!(h["group_sequence"], 0, "fresh session, delta does not bump");
        assert_eq!(h["payload_size"], 1);
        assert!(streaming::streaming_moq_publish_object(
            "sid1".to_string(),
            "pk".to_string(),
            1,
            false,
            "zz".to_string(),
        )
        .is_err());
    }
    #[test]
    fn streaming_moq_subscribe_status() {
        let res =
            streaming::streaming_moq_subscribe_stream("ab".repeat(32), "subscriber1".to_string())
                .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&res).unwrap();
        assert_eq!(parsed["protocol"], "MediaOverQUIC");
        assert_eq!(parsed["stream_id"], "ab".repeat(32));
        assert_eq!(
            parsed["status"], "unknown",
            "no stream registered -> deterministic unknown"
        );
    }
    #[test]
    fn streaming_get_video_url_uninitialized() {
        let e = streaming::streaming_get_video_url(
            "vid1".to_string(),
            "/tmp/nonexistent.mp4".to_string(),
        )
        .unwrap_err();
        assert!(e.contains("not initialized"), "got {e}");
    }
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn streaming_get_video_url_happy_path() {
        let _g = crate::test_util::lock();
        let port = streaming::streaming_start_local_server().await.unwrap();
        let url = streaming::streaming_get_video_url(
            "vid1".to_string(),
            "/tmp/nonexistent.mp4".to_string(),
        )
        .unwrap();
        assert!(url.contains(&format!("127.0.0.1:{port}")), "url {url}");
        assert!(url.contains("vid1"), "url {url}");
        assert!(streaming::streaming_get_video_url(
            "vid2".to_string(),
            "/tmp/other.mp4".to_string(),
        )
        .is_ok());
    }
}
