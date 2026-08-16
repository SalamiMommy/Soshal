// Tests for Zap (NWC/LNURL) and WebRTC FFI bridge modules
// Run with: cargo test -p soshal-flutter-bridge --test ffi_zap_webrtc_tests

#[cfg(test)]
mod ffi_tests {
    use soshal_flutter_bridge::*;

    // Serializes tests that touch the shared in-process NWC state or the
    // shared DB handle (statics are process-global across parallel tests).
    static ZAP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn init_db(name: &str) -> String {
        let path = soshal_test_util::tmp_path("zap_webrtc", &format!("{name}.db"))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        db::db_init(path.clone()).unwrap();
        path
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    // --- zap: DB-backed fns (no db_init in this binary -> deterministic
    //     "database not initialized" error) --------------------------------

    #[test]
    fn zap_ffi_fetch_receipts_bad_limit() {
        for limit in [0, -1, 501] {
            let r = zap::zap_fetch_receipts("event1".to_string(), limit);
            let e = r.err().unwrap();
            assert!(e.contains("limit"));
        }
    }

    #[test]
    fn zap_ffi_fetch_receipts_uninitialized_db() {
        let r = zap::zap_fetch_receipts("event1".to_string(), 10);
        let e = r.err().unwrap();
        assert!(e.contains("database not initialized"), "got {e}");
    }

    #[test]
    fn zap_ffi_total_msat_uninitialized_db() {
        let r = zap::zap_get_total_msat("event1".to_string());
        let e = r.err().unwrap();
        assert!(e.contains("database not initialized"), "got {e}");
    }

    #[test]
    fn zap_ffi_receipts_and_total_happy_path() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("zap_happy");
        let seed = |id: &str, event: &str, amount: i64, created: i64| {
            let sql = format!(
                "INSERT INTO zaps (id, event_id, recipient_pubkey, sender_pubkey, amount_msat, \
                 comment, created_at, pubkey, amount, content, zap_type) VALUES \
                 ('{id}', '{event}', 'recv', 'send', {}, 'thanks', {created}, 'send', {}, 'note', 'public')",
                amount * 1000,
                amount
            );
            db::db_query_raw(sql).unwrap();
        };
        seed("z1", "evt-1", 1000, 100);
        seed("z2", "evt-1", 2000, 200);
        seed("z_other", "evt-9", 9999, 300);

        let json = zap::zap_fetch_receipts("evt-1".to_string(), 10).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2, "only evt-1 rows");
        assert_eq!(v[0]["id"], "z2", "newest first (created_at DESC)");
        assert_eq!(v[1]["id"], "z1");
        assert_eq!(v[0]["amount"], 2000);

        assert_eq!(zap::zap_get_total_msat("evt-1".to_string()).unwrap(), 3000);
        assert_eq!(zap::zap_get_total_msat("missing".to_string()).unwrap(), 0);
        cleanup(&path);
    }

    // --- webrtc: ICE configuration ----------------------------------------

    #[test]
    fn webrtc_ffi_get_ice_config_public() {
        let json = webrtc::webrtc_get_ice_config("public".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "all");
        assert_eq!(v["forceRelay"], false);
        assert_eq!(v["iceServers"][0]["urls"], "stun:stun.l.google.com:19302");
    }

    #[test]
    fn webrtc_ffi_get_ice_config_friends() {
        let json = webrtc::webrtc_get_ice_config("friends".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "relay");
        assert_eq!(v["forceRelay"], true);
    }

    #[test]
    fn webrtc_ffi_get_ice_config_empty_level_defaults_all() {
        let json = webrtc::webrtc_get_ice_config(String::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "all");
        assert_eq!(v["forceRelay"], false);
    }

    #[test]
    fn webrtc_ffi_get_stun_servers() {
        assert_eq!(
            webrtc::webrtc_get_stun_servers().unwrap(),
            vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ]
        );
    }

    #[test]
    fn webrtc_ffi_get_turn_servers_unconfigured_errs() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("webrtc_turn_empty");
        let expected = "turn provisioning unavailable: no turn_endpoint configured (server endpoint on roadmap)".to_string();
        assert_eq!(webrtc::webrtc_get_turn_servers(None).unwrap_err(), expected);
        assert_eq!(
            webrtc::webrtc_get_turn_servers(Some("token".to_string())).unwrap_err(),
            expected
        );
        cleanup(&path);
    }

    #[test]
    fn webrtc_ffi_get_turn_servers_from_settings() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = init_db("webrtc_turn");
        assert!(db::db_set_setting(
            "turn_endpoint".to_string(),
            "turn:turn.example.com:3478".to_string()
        )
        .unwrap());
        assert!(db::db_set_setting("turn_username".to_string(), "u1".to_string()).unwrap());
        assert!(db::db_set_setting("turn_credential".to_string(), "s3cret".to_string()).unwrap());
        let json = webrtc::webrtc_get_turn_servers(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v[0]["urls"][0], "turn:turn.example.com:3478");
        assert_eq!(v[0]["username"], "u1");
        assert_eq!(v[0]["credential"], "s3cret");
        assert_eq!(v[0]["credentialType"], "password");
        cleanup(&path);
    }

    #[test]
    fn webrtc_ffi_create_peer_config() {
        let json = webrtc::webrtc_create_peer_config("public".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["iceTransportPolicy"], "all");
        assert_eq!(v["bundlePolicy"], "max-bundle");
        assert_eq!(v["rtcpMuxPolicy"], "require");
        assert!(!v["iceServers"].as_array().unwrap().is_empty());

        let friends = webrtc::webrtc_create_peer_config("friends".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&friends).unwrap();
        assert_eq!(v["iceTransportPolicy"], "relay");
    }

    // --- webrtc: SDP helpers ----------------------------------------------

    #[test]
    fn webrtc_ffi_extract_candidates() {
        let sdp = "v=0\r\na=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\r\nm=audio 0 RTP/AVP 0\r\na=candidate:2 1 UDP 2130706431 66.154.114.51 54322 typ relay".to_string();
        assert_eq!(
            webrtc::webrtc_extract_candidates(sdp).unwrap(),
            vec![
                "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx".to_string(),
                "a=candidate:2 1 UDP 2130706431 66.154.114.51 54322 typ relay".to_string(),
            ]
        );
        assert_eq!(
            webrtc::webrtc_extract_candidates("v=0\nm=audio 0 RTP/AVP 0".to_string()).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn webrtc_ffi_add_candidate_to_sdp() {
        assert_eq!(
            webrtc::webrtc_add_candidate_to_sdp("v=0".to_string(), "a=candidate:1".to_string())
                .unwrap(),
            "v=0\na=candidate:1"
        );
        assert_eq!(
            webrtc::webrtc_add_candidate_to_sdp("v=0\n".to_string(), "a=candidate:2".to_string())
                .unwrap(),
            "v=0\na=candidate:2"
        );
    }

    #[test]
    fn webrtc_ffi_validate_sdp() {
        assert!(
            webrtc::webrtc_validate_sdp("v=0\r\no=- 0 0 IN IP4 127.0.0.1".to_string()).unwrap()
        );
        assert!(!webrtc::webrtc_validate_sdp("v=0 only".to_string()).unwrap());
        assert!(!webrtc::webrtc_validate_sdp("garbage".to_string()).unwrap());
    }

    #[test]
    fn webrtc_ffi_sanitize_sdp_drops_host_candidates() {
        assert_eq!(
            webrtc::webrtc_sanitize_sdp(
                "a=candidate:1 1 UDP 2130706431 192.168.1.5 54321 typ host".to_string(),
                false
            )
            .unwrap(),
            ""
        );
        assert_eq!(
            webrtc::webrtc_sanitize_sdp(
                "a=candidate:1 1 UDP 2130706431 192.168.1.5 54321 typ host".to_string(),
                true
            )
            .unwrap(),
            ""
        );
    }

    #[test]
    fn webrtc_ffi_sanitize_sdp_srflx_gated_by_force_relay() {
        let srflx = "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx".to_string();
        assert_eq!(
            webrtc::webrtc_sanitize_sdp(srflx.clone(), false).unwrap(),
            srflx.clone().replace("\r\n", "")
        );
        assert_eq!(webrtc::webrtc_sanitize_sdp(srflx, true).unwrap(), "");
    }

    #[test]
    fn webrtc_ffi_sanitize_sdp_keeps_relay_and_private_srflx() {
        let relay = "a=candidate:2 1 UDP 2130706431 66.154.114.51 54322 typ relay".to_string();
        assert_eq!(
            webrtc::webrtc_sanitize_sdp(relay.clone(), true).unwrap(),
            relay.replace("\r\n", "")
        );
        assert_eq!(
            webrtc::webrtc_sanitize_sdp(
                "a=candidate:3 1 UDP 2130706431 10.0.0.5 54323 typ srflx".to_string(),
                false
            )
            .unwrap(),
            ""
        );
    }

    #[test]
    fn webrtc_ffi_sanitize_sdp_rewrites_private_c_lines() {
        assert_eq!(
            webrtc::webrtc_sanitize_sdp("c=IN IP4 192.168.1.5".to_string(), false).unwrap(),
            "c=IN IP4 127.0.0.1"
        );
        assert_eq!(
            webrtc::webrtc_sanitize_sdp("c=IN IP4 8.8.8.8".to_string(), false).unwrap(),
            "c=IN IP4 8.8.8.8"
        );
    }

    #[test]
    fn webrtc_ffi_sanitize_sdp_rewrites_origin_address() {
        assert_eq!(
            webrtc::webrtc_sanitize_sdp("o=- 0 0 IN IP4 192.168.1.5".to_string(), false).unwrap(),
            "o=o=- 0 0 IN IP4 0.0.0.0"
        );
    }

    #[test]
    fn webrtc_ffi_sanitize_sdp_joins_lines_crlf() {
        assert_eq!(
            webrtc::webrtc_sanitize_sdp(
                "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\nc=IN IP4 192.168.1.5"
                    .to_string(),
                false
            )
            .unwrap(),
            "a=candidate:1 1 UDP 2130706431 8.8.8.8 54321 typ srflx\r\nc=IN IP4 127.0.0.1"
        );
    }

    // --- zap: NWC lifecycle + LNURL parsing (no live NWC exchange) --------
    // NWC state is process-global (statics in the lib) -> ZAP_TEST_LOCK.

    #[test]
    fn zap_ffi_parse_lnurl_metadata_valid() {
        let json = zap::zap_parse_lnurl_metadata("alice@example.com".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "alice");
        assert_eq!(v["domain"], "example.com");
        assert_eq!(
            v["callback"],
            "https://example.com/.well-known/lnurlp/alice"
        );
        let json = zap::zap_parse_lnurl_metadata("bob_1.x@sub.example.org".to_string()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "bob_1.x");
        assert_eq!(v["domain"], "sub.example.org");
        assert!(v["callback"].as_str().unwrap().starts_with("https://"));
    }

    #[test]
    fn zap_ffi_parse_lnurl_metadata_rejects_malformed() {
        for bad in [
            String::new(),
            "not-an-address".to_string(),
            "@example.com".to_string(),
            "user@".to_string(),
            "../admin@example.com".to_string(),
            "us er@example.com".to_string(),
            format!("{}@example.com", "a".repeat(65)),
        ] {
            assert!(
                zap::zap_parse_lnurl_metadata(bad.clone()).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn zap_ffi_connect_nwc_rejects_invalid_uri() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = zap::zap_disconnect_nwc();
        let e = zap::zap_connect_nwc("not-a-wallet-connect-uri".to_string()).unwrap_err();
        assert!(e.contains("invalid NWC URI"), "got {e}");
        let long = format!("nostr+walletconnect://{}", "a".repeat(5000));
        let e = zap::zap_connect_nwc(long).unwrap_err();
        assert!(e.contains("NWC URI too long"), "got {e}");
        // failed connects never leave NWC state behind
        assert!(zap::zap_get_nwc_pubkey().is_err());
    }

    #[test]
    fn zap_ffi_nwc_connect_status_pubkey_never_leaks_secret() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = zap::zap_disconnect_nwc();
        let status = zap::zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], false);
        assert!(zap::zap_get_nwc_pubkey().is_err());

        assert!(zap::zap_connect_nwc(zap::NWC_URI.to_string()).unwrap());
        let status = zap::zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], true);
        assert_eq!(
            v["wallet_pubkey"],
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        );
        // Security invariant: the NWC secret never leaves Rust.
        assert!(
            !status.contains("f0f0f0f0"),
            "status leaked NWC secret material"
        );
        assert_eq!(
            zap::zap_get_nwc_pubkey().unwrap(),
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        );

        assert!(zap::zap_disconnect_nwc().unwrap());
        let status = zap::zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], false);
        assert!(zap::zap_get_nwc_pubkey().is_err());
    }

    // Lock held across the await deliberately: ZAP_TEST_LOCK serializes the
    // process-global NWC state so no other test can mutate it mid-flight.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn zap_ffi_fetch_invoice_error_paths_no_network() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = zap::zap_disconnect_nwc();
        // invalid lud16 -> parse error before any NWC state
        let e = zap::zap_fetch_invoice(
            "not-an-address".to_string(),
            1000,
            String::new(),
            String::new(),
        )
        .await
        .unwrap_err();
        assert!(e.contains("LNURL parse failed"), "got {e}");
        // zero amount -> rejected before any NWC state
        let e = zap::zap_fetch_invoice(
            "bob@example.com".to_string(),
            0,
            String::new(),
            String::new(),
        )
        .await
        .unwrap_err();
        assert!(e.contains("amount must be positive"), "got {e}");
        // no NWC connection -> fails before any relay exchange
        let e = zap::zap_fetch_invoice(
            "bob@example.com".to_string(),
            1000,
            "thanks".to_string(),
            String::new(),
        )
        .await
        .unwrap_err();
        assert!(e.contains("NWC not connected"), "got {e}");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn zap_ffi_send_payment_error_paths_no_network() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = zap::zap_disconnect_nwc();
        let e = zap::zap_send_payment("lnbc1fake".to_string())
            .await
            .unwrap_err();
        assert!(e.contains("NWC not connected"), "got {e}");
    }
}
