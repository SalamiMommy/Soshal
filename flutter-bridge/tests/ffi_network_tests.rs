#[path = "common/mod.rs"]
mod test_util;

#[cfg(test)]
mod network_ffi_tests {
    use soshal_flutter_bridge::*;
    #[test]
    fn network_ffi_transport_mode_roundtrip() {
        let _g = crate::test_util::lock();
        for (set, expect) in [("clearnet", "nostr"), ("auto", "default"), ("i2p", "i2p")] {
            assert!(network::network_set_transport_mode(set.to_string()).unwrap());
            let got = network::network_get_transport_mode().unwrap();
            assert_eq!(got, expect, "set {set}, got {got}");
        }
        network::network_set_transport_mode("default".to_string()).unwrap();
    }
    #[test]
    fn network_ffi_transport_mode_invalid_rejected() {
        let _g = crate::test_util::lock();
        network::network_set_transport_mode("clearnet".to_string()).unwrap();
        assert!(network::network_set_transport_mode("quantum".to_string()).is_err());
        assert_eq!(network::network_get_transport_mode().unwrap(), "nostr");
    }
    #[test]
    fn network_ffi_get_sys_diagnostics_ok() {
        let diag = network::network_get_sys_diagnostics().unwrap();
        assert!(!diag.is_empty());
        assert!(diag.contains("schema_version"));
    }
    #[test]
    fn network_ffi_notify_interface_change_valid() {
        assert!(network::network_notify_interface_change("192.168.1.50:8080".to_string()).unwrap());
    }
    #[test]
    fn network_ffi_notify_interface_change_invalid() {
        let e = network::network_notify_interface_change("not-an-ip".to_string()).unwrap_err();
        assert!(e.contains("Invalid IP address format"), "got {e}");
    }
    #[test]
    fn network_ffi_verify_zk_wot_proof_garbage_rejected() {
        let result = network::network_verify_zk_wot_proof(
            "not json".to_string(),
            "wot_root_123".to_string(),
            "[]".to_string(),
        )
        .unwrap();
        assert!(!result);
    }
    #[test]
    fn network_ffi_verify_zk_wot_proof_valid() {
        let proof = soshal_crypto_core::zk_trust::generate_zk_wot_proof(
            "pubkey_alice",
            "wot_root_123",
            "black_root_456",
        );
        let proof_json = serde_json::to_string(&proof).unwrap();
        let result = network::network_verify_zk_wot_proof(
            proof_json,
            "wot_root_123".to_string(),
            "[]".to_string(),
        )
        .unwrap();
        assert!(result);
    }
    #[test]
    fn network_ffi_verify_zk_wot_proof_wrong_root_rejected() {
        let proof = soshal_crypto_core::zk_trust::generate_zk_wot_proof(
            "pubkey_alice",
            "wot_root_123",
            "black_root_456",
        );
        let proof_json = serde_json::to_string(&proof).unwrap();
        let result = network::network_verify_zk_wot_proof(
            proof_json,
            "different_root".to_string(),
            "[]".to_string(),
        )
        .unwrap();
        assert!(!result);
    }
    #[test]
    fn network_ffi_i2p_session_status_inert() {
        let status = network::i2p_session_status().unwrap();
        assert!(status.contains("\"running\":false"));
    }
    #[test]
    fn network_ffi_i2p_stop_session_inert() {
        assert!(!network::i2p_stop_session().unwrap());
        let status = network::i2p_session_status().unwrap();
        assert!(status.contains("\"running\":false"), "got {status}");
    }
    #[test]
    fn network_ffi_reconcile_prolly_tree_matching_roots() {
        let kv = "[[\"a\",\"1\"],[\"b\",\"2\"]]".to_string();
        let tree = soshal_sync_core::prolly_tree::ProllyTree::build(&[
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]);
        let resp = network::network_reconcile_prolly_tree(kv, tree.root_hash.clone()).unwrap();
        assert_eq!(resp, "\"Match\"", "equal roots -> Match");
    }
    #[test]
    fn network_ffi_reconcile_prolly_tree_divergent_roots() {
        let kv = "[[\"a\",\"1\"],[\"b\",\"2\"]]".to_string();
        let other = soshal_sync_core::prolly_tree::ProllyTree::build(&[
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "9".to_string()),
        ]);
        let resp = network::network_reconcile_prolly_tree(kv, other.root_hash).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(
            v["RequestBranch"]["level"] == 0,
            "divergent roots -> RequestBranch, got {resp}"
        );
        assert!(!v["RequestBranch"]["node_hash"].as_str().unwrap().is_empty());
    }
    #[test]
    fn network_ffi_reticulum_status_not_running() {
        network::network_reticulum_stop().unwrap();
        let status = network::network_reticulum_status().unwrap();
        assert!(status.contains("\"running\":false"));
    }
    #[test]
    fn network_ffi_reticulum_stop_inert() {
        assert!(network::network_reticulum_stop().unwrap());
        assert!(
            network::network_reticulum_stop().unwrap(),
            "double stop idempotent"
        );
        let status = network::network_reticulum_status().unwrap();
        assert!(status.contains("\"running\":false"), "got {status}");
    }
    #[test]
    fn network_ffi_reticulum_address_from_pubkey() {
        let addr: serde_json::Value = serde_json::from_str(
            &network::reticulum_address_from_pubkey("test-pubkey".to_string()).unwrap(),
        )
        .unwrap();
        let bytes = addr.as_array().unwrap();
        assert_eq!(bytes.len(), 16, "16-byte RNS address");
        let again: serde_json::Value = serde_json::from_str(
            &network::reticulum_address_from_pubkey("test-pubkey".to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(addr, again, "deterministic derivation");
    }
    #[test]
    fn network_ffi_reticulum_address_from_aspect() {
        let a: serde_json::Value = serde_json::from_str(
            &network::reticulum_address_from_aspect("soshal".to_string(), "default".to_string())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(a.as_array().unwrap().len(), 16);
        let b: serde_json::Value = serde_json::from_str(
            &network::reticulum_address_from_aspect("soshal".to_string(), "dm".to_string())
                .unwrap(),
        )
        .unwrap();
        assert_ne!(a, b, "distinct aspects -> distinct addresses");
    }
}
