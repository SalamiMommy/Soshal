// Tests for Zap (NWC/LNURL) and WebRTC FFI bridge modules
// Run with: cargo test -p soshal-flutter-bridge --test ffi_zap_webrtc_tests

#[cfg(test)]
mod ffi_tests {
    use soshal_flutter_bridge::*;

    // Serializes tests that touch the shared in-process NWC state or the
    // shared DB handle (statics are process-global across parallel tests).
    static ZAP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const NWC_PUBKEY: &str = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    const NWC_SECRET: &str = "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";
    const NWC_URI: &str = "nostr+walletconnect://abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789?relay=wss://relay.damus.io&secret=f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";

    // --- zap: LNURL parsing ------------------------------------------------

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
    }

    #[test]
    fn zap_ffi_parse_lnurl_metadata_invalid() {
        let no_at = zap::zap_parse_lnurl_metadata("not-an-address".to_string());
        assert!(no_at.is_err());
        let empty_domain = zap::zap_parse_lnurl_metadata("user@".to_string());
        assert!(empty_domain.is_err());
        let traversal = zap::zap_parse_lnurl_metadata("../admin@example.com".to_string());
        assert!(traversal.is_err());
    }

    // --- zap: NWC connect/disconnect/status (in-process state only; the
    //     connect path stores the URI Rust-side and opens no connection) ----

    #[test]
    fn zap_ffi_connect_nwc_invalid_uri() {
        let garbage = zap::zap_connect_nwc("not a uri".to_string());
        assert!(garbage.is_err());
        let long = zap::zap_connect_nwc("x".repeat(5000));
        let e = long.err().unwrap();
        assert!(e.contains("too long"));
    }

    #[test]
    fn zap_ffi_connect_nwc_rejects_cleartext_relay() {
        let uri = format!(
            "nostr+walletconnect://{NWC_PUBKEY}?relay=ws://relay.example.com&secret={NWC_SECRET}"
        );
        let r = zap::zap_connect_nwc(uri);
        let e = r.err().unwrap();
        assert!(e.contains("wss"));
    }

    #[test]
    fn zap_ffi_connect_nwc_roundtrip() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(zap::zap_connect_nwc(NWC_URI.to_string()).unwrap());

        let status = zap::zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], true);
        assert_eq!(v["wallet_pubkey"], NWC_PUBKEY);
        assert_eq!(zap::zap_get_nwc_pubkey().unwrap(), NWC_PUBKEY);

        assert!(zap::zap_disconnect_nwc().unwrap());
        let status = zap::zap_get_nwc_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(v["connected"], false);
        let e = zap::zap_get_nwc_pubkey().err().unwrap();
        assert!(!e.is_empty());
    }

    #[test]
    fn zap_ffi_disconnect_returns_ok_when_not_connected() {
        let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = zap::zap_disconnect_nwc();
        assert!(zap::zap_disconnect_nwc().unwrap());
    }

    // --- zap: network-gated fns probe only the failure-before-connect path
    //     (disconnected state); never opens an NWC relay connection --------

    #[tokio::test]
    async fn zap_ffi_fetch_invoice_fails_before_connect() {
        {
            let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let _ = zap::zap_disconnect_nwc();
        }
        let bad_lnurl = zap::zap_fetch_invoice(
            "not-an-address".to_string(),
            1000,
            String::new(),
            String::new(),
        )
        .await;
        let e = bad_lnurl.err().unwrap();
        assert!(e.contains("LNURL parse failed"));

        let zero_amount = zap::zap_fetch_invoice(
            "bob@example.com".to_string(),
            0,
            String::new(),
            String::new(),
        )
        .await;
        let e = zero_amount.err().unwrap();
        assert!(e.contains("amount must be positive"));

        let disconnected = zap::zap_fetch_invoice(
            "bob@example.com".to_string(),
            1000,
            String::new(),
            String::new(),
        )
        .await;
        let e = disconnected.err().unwrap();
        assert!(e.contains("NWC not connected"));
    }

    #[tokio::test]
    async fn zap_ffi_send_payment_fails_when_disconnected() {
        {
            let _g = ZAP_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let _ = zap::zap_disconnect_nwc();
        }
        let r = zap::zap_send_payment("lnbc1fake".to_string()).await;
        let e = r.err().unwrap();
        assert!(e.contains("NWC not connected"));
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
        assert!(!e.is_empty());
    }

    #[test]
    fn zap_ffi_total_msat_uninitialized_db() {
        let r = zap::zap_get_total_msat("event1".to_string());
        let e = r.err().unwrap();
        assert!(!e.is_empty());
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
    fn webrtc_ffi_get_turn_servers() {
        assert_eq!(webrtc::webrtc_get_turn_servers(None).unwrap(), "[]");
        let r = webrtc::webrtc_get_turn_servers(Some("token".to_string()));
        let e = r.err().unwrap();
        assert!(e.contains("TURN provisioning"));
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
}
