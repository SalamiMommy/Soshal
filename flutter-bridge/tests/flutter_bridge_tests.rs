// Tests for Flutter FFI bridge
// Run with: cargo test --test flutter_bridge_tests

#[cfg(test)]
mod ffi_tests {
    use soshal_flutter_bridge::*;

    #[test]
    fn test_auth_generate_mnemonic() {
        let result = auth::auth_generate_mnemonic().unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_auth_validate_mnemonic() {
        let mnemonic = auth::auth_generate_mnemonic().unwrap();
        let valid = auth::auth_validate_mnemonic(mnemonic).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_auth_validate_invalid_mnemonic() {
        let valid =
            auth::auth_validate_mnemonic("not a valid mnemonic phrase".to_string()).unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_auth_keypair_generation() {
        let keypair: KeyPairResult =
            serde_json::from_str(&auth::auth_generate_keypair().unwrap()).unwrap();
        assert_eq!(keypair.public_key.len(), 64, "pubkey must be 64 hex chars");
        assert!(keypair.public_key.chars().all(|c| c.is_ascii_hexdigit()));
        // Security invariant: secret key material never crosses FFI.
        assert!(
            keypair.secret_key.is_empty(),
            "secret key must stay blank across the bridge"
        );
    }

    #[test]
    fn test_auth_npub_encode_decode() {
        let keypair: KeyPairResult =
            serde_json::from_str(&auth::auth_generate_keypair().unwrap()).unwrap();
        let npub = auth::auth_npub_encode(keypair.public_key.clone()).unwrap();
        assert!(npub.starts_with("npub1"));
        let decoded = auth::auth_npub_decode(npub).unwrap();
        assert_eq!(decoded, keypair.public_key);
    }

    #[test]
    fn test_crypto_sha256_hex() {
        let hash = crypto::crypto_sha256_hex("hello world".to_string()).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_crypto_random_bytes() {
        let bytes = crypto::crypto_random_bytes(32).unwrap();
        assert_eq!(bytes.len(), 64);
        let bytes2 = crypto::crypto_random_bytes(32).unwrap();
        assert_ne!(bytes, bytes2);
    }

    #[test]
    fn test_util_base64url_encode_decode() {
        let encoded = util::util_base64url_encode("hello world".to_string()).unwrap();
        let decoded = util::util_base64url_decode(encoded).unwrap();
        assert_eq!(decoded, "hello world");
    }

    #[test]
    fn test_util_extract_hashtags() {
        let tags =
            util::util_extract_hashtags("Hello #world and #rust #soshal!".to_string()).unwrap();
        assert!(tags.contains(&"world".to_string()));
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"soshal".to_string()));
    }

    #[test]
    fn test_ffi_error_propagation() {
        // Exercise real FFI error paths: Result<T, String> must surface the
        // Rust-side error string to the caller.
        let err = db::db_path().unwrap_err();
        assert!(err.contains("database not initialized"), "got {err}");
        let missing = std::env::temp_dir().join("soshal-no-such-media-file.bin");
        let err2 = media::media_load_local(missing.to_string_lossy().to_string()).unwrap_err();
        assert!(!err2.is_empty());
    }
}

// Integration tests (requires real setup)
#[cfg(test)]
mod integration_tests {
    use soshal_flutter_bridge::*;

    #[tokio::test]
    async fn test_auth_flow_end_to_end() {
        let mnemonic = auth::auth_generate_mnemonic().unwrap();
        let restored_kp: KeyPairResult = serde_json::from_str(
            &auth::auth_restore_from_mnemonic(mnemonic, "".to_string())
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(!restored_kp.public_key.is_empty());
        let npub = auth::auth_npub_encode(restored_kp.public_key.clone()).unwrap();
        assert!(npub.starts_with("npub1"));
    }

    #[test]
    fn test_database_workflow() {
        let path = format!(
            "{}/soshal_bridge_flow_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        db::db_init(path.clone()).unwrap();
        let diag = db::db_query_raw("SELECT COUNT(*) AS n FROM sqlite_master".to_string()).unwrap();
        assert!(diag.contains("\"n\":"));
        let v: serde_json::Value = serde_json::from_str(&diag).unwrap();
        let n = v[0]["n"].as_i64().unwrap_or(0);
        assert!(n > 10, "expected schema tables, got {n}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[tokio::test]
    #[ignore] // Requires live network relays (wss://relay.damus.io, wss://nos.lol)
    async fn test_relay_pool_workflow() {
        let relays = vec![
            "wss://relay.damus.io".to_string(),
            "wss://nos.lol".to_string(),
        ];
        let _ = network::network_init_relays(relays).await.unwrap();
        let _ = network::network_get_relay_status().await.unwrap();
    }

    // --- P2P (LAN chunk server + swarm downloads + power scheduler) -------

    static P2P_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static P2P_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn tmp_p2p_root(label: &str) -> std::path::PathBuf {
        let n = P2P_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "soshal_bridge_p2p_{label}_{}_{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_p2p_power_scheduler_transitions() {
        let _g = P2P_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let full = p2p::p2p_power_update(true, 100, false, false).unwrap();
        assert_eq!(full.mode, "full");
        assert!(!full.paused);
        assert_eq!(full.max_parallel_uploads, 8);

        let paused = p2p::p2p_power_update(false, 40, true, true).unwrap();
        assert_eq!(paused.mode, "paused");
        assert!(paused.paused);
        assert_eq!(paused.max_parallel_uploads, 0);

        let throttled = p2p::p2p_power_update(false, 60, false, false).unwrap();
        assert_eq!(throttled.mode, "throttled");
        assert!(!throttled.paused);
        assert!(throttled.max_parallel_uploads < 8);
    }

    #[test]
    fn test_p2p_lan_server_requires_unlocked_signer() {
        let _g = P2P_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = p2p::p2p_lan_server_stop();
        signer::signer_lock().unwrap();
        assert!(p2p::p2p_lan_server_start(String::new()).is_err());
        assert!(p2p::p2p_swarm_download(
            "{}".to_string(),
            "[]".to_string(),
            "[]".to_string(),
            String::new(),
            1
        )
        .is_err());
    }

    #[test]
    fn test_p2p_lan_swarm_loopback_roundtrip() {
        let _g = P2P_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = p2p::p2p_stop_all();
        let keys = soshal_nostr_core::keys::generate_keys();
        signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        let root = tmp_p2p_root("store");
        let store = soshal_media_core::cas::ChunkStore::new(root.clone());
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
        let manifest = store.store_reader(std::io::Cursor::new(&data)).unwrap();
        store.save_manifest(&manifest).unwrap();
        let manifest_json = serde_json::to_string(&manifest).unwrap();

        let port = p2p::p2p_lan_server_start(root.to_string_lossy().to_string()).unwrap();
        assert!(port > 0);
        assert_eq!(port, p2p::p2p_lan_server_port().unwrap());

        let out_dir = tmp_p2p_root("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let out = out_dir.join("blob.bin");
        let id = p2p::p2p_swarm_download(
            manifest_json.clone(),
            format!("[\"127.0.0.1:{port}\"]"),
            "[null]".to_string(),
            out.to_string_lossy().to_string(),
            4,
        )
        .unwrap();

        let mut status = p2p::p2p_swarm_poll(id.clone()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while status.state != "done" && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
            status = p2p::p2p_swarm_poll(id.clone()).unwrap();
        }
        assert_eq!(status.state, "done", "swarm download timed out");
        assert_eq!(status.verified_chunks, manifest.chunks.len());
        assert_eq!(status.failures, 0);
        assert_eq!(status.bytes_downloaded as usize, data.len());

        let got = std::fs::read(&out).unwrap();
        assert_eq!(got.len(), data.len());
        assert_eq!(
            soshal_crypto_core::hash::blake3_hash_hex(&got),
            manifest.blob_hash
        );

        p2p::p2p_stop_all().unwrap();
        signer::signer_lock().unwrap();
    }
}
