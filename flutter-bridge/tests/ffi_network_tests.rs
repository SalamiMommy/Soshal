#[cfg(test)]
mod network_ffi_tests {
    use soshal_flutter_bridge::*;

    #[test]
    fn network_ffi_transport_mode_roundtrip() {
        for mode in ["clearnet", "auto", "i2p"] {
            assert!(network::network_set_transport_mode(mode.to_string()).unwrap());
            let got = network::network_get_transport_mode().unwrap();
            assert!(got == mode, "expected {mode}, got {got}");
        }
        network::network_set_transport_mode("clearnet".to_string()).unwrap();
    }

    #[test]
    fn network_ffi_transport_mode_invalid_rejected() {
        network::network_set_transport_mode("clearnet".to_string()).unwrap();
        assert!(network::network_set_transport_mode("quantum".to_string()).is_err());
        assert_eq!(network::network_get_transport_mode().unwrap(), "clearnet");
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
        assert!(network::network_notify_interface_change("not-an-ip".to_string()).is_err());
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
    }

    #[test]
    fn network_ffi_reconcile_prolly_tree_ok() {
        let resp = network::network_reconcile_prolly_tree(
            "[[\"a\",\"1\"],[\"b\",\"2\"]]".to_string(),
            "abc123".to_string(),
        )
        .unwrap();
        assert!(!resp.is_empty());
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
    }

    #[test]
    fn network_ffi_reticulum_address_from_pubkey() {
        let addr = network::reticulum_address_from_pubkey("test-pubkey".to_string()).unwrap();
        assert!(!addr.is_empty());
    }

    #[test]
    fn network_ffi_reticulum_address_from_aspect() {
        let addr =
            network::reticulum_address_from_aspect("soshal".to_string(), "default".to_string())
                .unwrap();
        assert!(!addr.is_empty());
    }
}
