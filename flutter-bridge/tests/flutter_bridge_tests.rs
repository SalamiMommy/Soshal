#[path = "common/mod.rs"]
pub mod test_util;

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
        // The secret key is not returned across FFI; the in-process signer
        // holds it directly, and backup happens via BIP-39 mnemonic.
        assert!(
            keypair.secret_key.is_empty(),
            "secret key must not leak across FFI"
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
        let hash = util::util_sha256_hex("hello world".to_string()).unwrap();
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
        let _g = crate::test_util::lock();
        let _ = db::db_close();
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
        let npub = auth::auth_npub_encode(restored_kp.public_key).unwrap();
        assert!(npub.starts_with("npub1"));
    }
    #[test]
    fn test_database_workflow() {
        let path = soshal_test_util::tmp_path("bridge_flow", "flow.db")
            .to_string_lossy()
            .to_string();
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
        let _g = P2P_TEST_LOCK.lock().unwrap();
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
        let _t = soshal_test_util::test_lock();
        let _g = P2P_TEST_LOCK.lock().unwrap();
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
        let _t = soshal_test_util::test_lock();
        let _g = P2P_TEST_LOCK.lock().unwrap();
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
            manifest_json,
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
    // --- signer: process-global key state (SIGNER static in the lib) ------
    //
    // Every test here mutates the process-global signer, so it holds BOTH the
    // repo-wide soshal_test_util::test_lock() AND the module P2P_TEST_LOCK
    // (the p2p tests above unlock/lock the same SIGNER under P2P_TEST_LOCK).
    // Fixed acquisition order: test_lock() first, P2P_TEST_LOCK second; the
    // p2p tests never take test_lock(), so there is no lock cycle.
    fn unlock_fresh_signer() -> (String, String) {
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        let secret = keys.secret_key().to_secret_hex();
        signer::signer_unlock(secret.clone()).unwrap();
        (pk, secret)
    }
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_signer_locked_error_paths() {
        let _t = soshal_test_util::test_lock();
        let _p = P2P_TEST_LOCK.lock().unwrap();
        signer::signer_lock().unwrap();
        assert!(signer::signer_is_locked().unwrap());
        assert!(signer::signer_pubkey()
            .unwrap_err()
            .contains("signer locked"));
        assert!(signer::signer_schnorr_sign("00".repeat(32))
            .unwrap_err()
            .contains("signer locked"));
        assert!(signer::signer_sign_text("hi".to_string())
            .unwrap_err()
            .contains("signer locked"));
        let unsigned =
            "{\"pubkey\":\"\",\"created_at\":0,\"kind\":1,\"tags\":[],\"content\":\"hi\"}"
                .to_string();
        assert!(signer::signer_sign_unsigned(unsigned)
            .unwrap_err()
            .contains("signer locked"));
        assert!(
            signer::signer_nip44_encrypt("secret".to_string(), "aa".repeat(32))
                .unwrap_err()
                .contains("signer locked")
        );
        assert!(
            signer::signer_nip44_decrypt("bad".to_string(), "aa".repeat(32))
                .unwrap_err()
                .contains("signer locked")
        );
        assert!(signer::signer_save_to_keyring("aa".repeat(32))
            .await
            .unwrap_err()
            .contains("signer locked"));
        // invalid secret never replaces the (empty) signer state
        let bad = signer::signer_unlock("not-a-secret-key".to_string()).unwrap_err();
        assert!(bad.contains("invalid secret key"), "got {bad}");
        assert!(signer::signer_is_locked().unwrap());
    }
    #[test]
    fn test_signer_unlock_pubkey_and_lock_roundtrip() {
        let _t = soshal_test_util::test_lock();
        let _p = P2P_TEST_LOCK.lock().unwrap();
        let (pk, _) = unlock_fresh_signer();
        assert!(!signer::signer_is_locked().unwrap());
        assert_eq!(signer::signer_pubkey().unwrap(), pk);
        assert!(signer::signer_lock().unwrap());
        assert!(signer::signer_is_locked().unwrap());
        assert!(signer::signer_pubkey().is_err());
    }
    #[test]
    fn test_signer_schnorr_and_text_signing() {
        let _t = soshal_test_util::test_lock();
        let _p = P2P_TEST_LOCK.lock().unwrap();
        let _ = unlock_fresh_signer();
        // 32-byte digest (64 hex chars) -> 64-byte schnorr sig (128 hex chars)
        let sig = signer::signer_schnorr_sign("42".repeat(32)).unwrap();
        assert_eq!(sig.len(), 128);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
        // text signing hashes SHA-256 internally, then signs
        let text_sig = signer::signer_sign_text("integration test".to_string()).unwrap();
        assert_eq!(text_sig.len(), 128);
        // bad hex and wrong digest length
        let e = signer::signer_schnorr_sign("zz".to_string()).unwrap_err();
        assert!(e.contains("invalid hex"), "got {e}");
        let e = signer::signer_schnorr_sign("00".repeat(16)).unwrap_err();
        assert!(e.contains("32 bytes"), "got {e}");
    }
    #[test]
    fn test_signer_sign_unsigned_event() {
        let _t = soshal_test_util::test_lock();
        let _p = P2P_TEST_LOCK.lock().unwrap();
        let (pk, _) = unlock_fresh_signer();
        let mut v: serde_json::Value = serde_json::from_str(
            "{\"pubkey\":\"\",\"created_at\":0,\"kind\":1,\"tags\":[],\"content\":\"hello\"}",
        )
        .unwrap();
        v["pubkey"] = serde_json::json!(pk);
        v["created_at"] = serde_json::json!(soshal_common_core::format::now_secs());
        let signed = signer::signer_sign_unsigned(v.to_string()).unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert_eq!(ev["pubkey"], v["pubkey"]);
        assert!(ev.get("id").is_some());
        assert!(ev.get("sig").is_some());
        assert_eq!(ev["sig"].as_str().unwrap().len(), 128);
        // malformed unsigned event JSON
        let e = signer::signer_sign_unsigned("not json".to_string()).unwrap_err();
        assert!(e.contains("invalid unsigned event"), "got {e}");
    }
    #[test]
    fn test_signer_nip44_encrypt_decrypt_roundtrip() {
        let _t = soshal_test_util::test_lock();
        let _p = P2P_TEST_LOCK.lock().unwrap();
        let alice = soshal_nostr_core::keys::generate_keys();
        let bob = soshal_nostr_core::keys::generate_keys();
        signer::signer_unlock(alice.secret_key().to_secret_hex()).unwrap();
        let payload =
            signer::signer_nip44_encrypt("secret dm".to_string(), bob.public_key().to_hex())
                .unwrap();
        // invalid recipient pubkey
        let e = signer::signer_nip44_encrypt("x".to_string(), "not-hex".to_string()).unwrap_err();
        assert!(e.contains("invalid recipient pubkey"), "got {e}");
        // decrypt as bob (unlock replaces alice's key in the SIGNER static)
        signer::signer_unlock(bob.secret_key().to_secret_hex()).unwrap();
        let plain =
            signer::signer_nip44_decrypt(payload.clone(), alice.public_key().to_hex()).unwrap();
        assert_eq!(plain.as_str(), "secret dm");
        // tampered payload -> decrypt fails
        let mut tampered = payload.into_bytes();
        let mid = tampered.len() / 2;
        tampered[mid] ^= 0x01;
        let e = signer::signer_nip44_decrypt(
            String::from_utf8(tampered).unwrap(),
            alice.public_key().to_hex(),
        )
        .unwrap_err();
        assert!(e.contains("nip44 decrypt"), "got {e}");
        signer::signer_lock().unwrap();
    }
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_signer_keyring_save_unlock_remove() {
        let _t = soshal_test_util::test_lock();
        let _p = P2P_TEST_LOCK.lock().unwrap();
        let (pk, _) = unlock_fresh_signer();
        // mismatched pubkey is rejected before any keychain access
        let e = signer::signer_save_to_keyring("bb".repeat(32))
            .await
            .unwrap_err();
        assert!(e.contains("pubkey does not match"), "got {e}");
        // unknown account: nothing stored -> Err (keychain missing or entry absent)
        assert!(signer::signer_unlock_from_keyring("cc".repeat(32))
            .await
            .is_err());
        // A locked/prompting keyring makes writes block indefinitely; probe
        // it with a bounded check and skip the roundtrip when unavailable
        // (headless CI, locked desktop keyring).
        if !signer::keyring_available() {
            eprintln!("SKIP: OS keyring unavailable (locked or headless)");
            signer::signer_lock().unwrap();
            return;
        }
        // save/unlock/remove roundtrip; keychain may be unavailable on
        // headless CI, so a non-empty Err is tolerated there
        match signer::signer_save_to_keyring(pk.clone()).await {
            Ok(true) => {
                signer::signer_lock().unwrap();
                assert!(signer::signer_is_locked().unwrap());
                assert!(signer::signer_unlock_from_keyring(pk.clone())
                    .await
                    .unwrap());
                assert!(!signer::signer_is_locked().unwrap());
                assert_eq!(signer::signer_pubkey().unwrap(), pk);
                match signer::signer_remove_from_keyring(pk.clone()) {
                    Ok(true) => assert!(signer::signer_unlock_from_keyring(pk).await.is_err()),
                    Ok(false) => {}
                    Err(e) => assert!(!e.is_empty()),
                }
            }
            Ok(false) => panic!("save_to_keyring returned false"),
            Err(e) => assert!(!e.is_empty(), "keychain error must be non-empty"),
        }
        signer::signer_lock().unwrap();
    }
}

#[path = "flutter_bridge_tests/bridge_gap.rs"]
mod bridge_gap;
#[path = "flutter_bridge_tests/ffi_aux_modules.rs"]
mod ffi_aux_modules;
#[path = "flutter_bridge_tests/ffi_coverage.rs"]
mod ffi_coverage;
#[path = "flutter_bridge_tests/ffi_dating_marketplace.rs"]
mod ffi_dating_marketplace;
#[path = "flutter_bridge_tests/ffi_identity.rs"]
mod ffi_identity;
#[path = "flutter_bridge_tests/ffi_media_streaming.rs"]
mod ffi_media_streaming;
#[path = "flutter_bridge_tests/ffi_moderation_turso.rs"]
mod ffi_moderation_turso;
#[path = "flutter_bridge_tests/ffi_more_gap.rs"]
mod ffi_more_gap;
#[path = "flutter_bridge_tests/ffi_network.rs"]
mod ffi_network;
#[path = "flutter_bridge_tests/ffi_zap_webrtc.rs"]
mod ffi_zap_webrtc;
#[path = "flutter_bridge_tests/identity_gap.rs"]
mod identity_gap;
#[path = "flutter_bridge_tests/protocol_handler_gap.rs"]
mod protocol_handler_gap;
#[path = "flutter_bridge_tests/saved_music_test.rs"]
mod saved_music_test;
