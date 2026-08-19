use soshal_network_core::http3_client::{Http3Client, HttpResponseData};
use soshal_network_core::multi_bearer::MultiBearerActor;
use soshal_network_core::plumtree::PlumTreeMessage;
use soshal_network_core::wifi_direct::{WifiDirectManager, WifiP2pConfig, WifiP2pStatus};

#[test]
fn multi_bearer_beacon_roundtrip() {
    let actor = MultiBearerActor::new("abcdef0123456789abcdef0123456789", [0u8; 32]);
    let beacon = actor.build_ble_state_root_beacon("00112233445566778899aabbccddeeff");
    assert!(beacon.starts_with("SOSHAL_abcdef012345"), "got {beacon}");
    assert!(
        beacon.ends_with("0011223344556677"),
        "root truncated to 16 chars: {beacon}"
    );
}

#[test]
fn multi_bearer_ble_beacon_diff_and_malformed() {
    let actor = MultiBearerActor::new("abcdef0123456789abcdef0123456789", [0u8; 32]);
    let beacon = actor.build_ble_state_root_beacon("0011223344556677");

    let rt = tokio::runtime::Runtime::new().unwrap();
    let same_root = rt.block_on(actor.process_ble_beacon(&beacon, "0011223344556677"));
    assert!(same_root.is_none(), "same root must not trigger upgrade");
    let diff_root = rt.block_on(actor.process_ble_beacon(&beacon, "ffffffffffffffff"));
    assert_eq!(diff_root.as_deref(), Some("abcdef012345"));
    let no_colon = rt.block_on(actor.process_ble_beacon("no-colon-here", "0011223344556677"));
    assert!(no_colon.is_none());
    let unknown_device =
        rt.block_on(actor.process_ble_beacon("SOSHAL_zzzz:0011223344556677", "ffffffffffffffff"));
    assert!(
        unknown_device.is_none(),
        "unparseable device name yields no peer"
    );
}

#[test]
fn multi_bearer_get_state_initial() {
    let actor = MultiBearerActor::new("abcdef0123456789abcdef0123456789", [0u8; 32]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let state = rt.block_on(actor.get_state());
    assert_eq!(state.own_pubkey, "abcdef0123456789abcdef0123456789");
    assert!(state.ble_active);
    assert!(!state.wifi_direct_connected);
    assert_eq!(state.active_peer_count, 0);
}

#[test]
fn multi_bearer_routes_plumtree_gossip() {
    let actor = MultiBearerActor::new("abcdef0123456789abcdef0123456789", [0u8; 32]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let msg = PlumTreeMessage::IHave {
        message_id: "m1".into(),
        round: 1,
    };
    let outgoing = rt.block_on(actor.route_plumtree_gossip("peer_a", msg.clone()));
    assert_eq!(outgoing.len(), 1, "unknown IHave triggers Graft to sender");
    assert!(matches!(outgoing[0].1, PlumTreeMessage::Graft { .. }));
    assert_eq!(outgoing[0].0, "peer_a");
    let dup = rt.block_on(actor.route_plumtree_gossip("peer_a", msg));
    assert!(dup.is_empty());
}

#[test]
fn wifi_direct_status_lifecycle() {
    let manager = WifiDirectManager::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    assert_eq!(
        rt.block_on(manager.get_status("ghost")),
        WifiP2pStatus::Disconnected
    );
    rt.block_on(manager.set_status("p1", WifiP2pStatus::Negotiating));
    assert_eq!(
        rt.block_on(manager.get_status("p1")),
        WifiP2pStatus::Negotiating
    );
    rt.block_on(manager.set_status(
        "p1",
        WifiP2pStatus::Connected {
            peer_ip: "192.168.49.2".into(),
            speed_mbps: 50,
        },
    ));
    assert_eq!(
        rt.block_on(manager.get_status("p1")),
        WifiP2pStatus::Connected {
            peer_ip: "192.168.49.2".into(),
            speed_mbps: 50,
        }
    );
    rt.block_on(manager.set_status("p1", WifiP2pStatus::Failed("timeout".into())));
    assert_eq!(
        rt.block_on(manager.get_status("p1")),
        WifiP2pStatus::Failed("timeout".into())
    );
}

#[test]
fn wifi_direct_chunk_frame_binary_roundtrip() {
    let binary: Vec<u8> = (0u8..=255).collect();
    let frame = WifiDirectManager::format_chunk_frame("hash1", 3, 10, &binary);
    let parsed = WifiDirectManager::parse_chunk_frame(&frame).unwrap();
    assert_eq!(parsed.0, "hash1");
    assert_eq!(parsed.1, 3);
    assert_eq!(parsed.2, 10);
    assert_eq!(parsed.3, binary, "binary payload survives base64 roundtrip");
}

#[test]
fn wifi_direct_chunk_frame_malformed() {
    assert!(WifiDirectManager::parse_chunk_frame("not json").is_none());
    assert!(WifiDirectManager::parse_chunk_frame("{}").is_none());
    assert!(WifiDirectManager::parse_chunk_frame(
        r#"{"chunk_hash":"h","chunk_index":0,"total_chunks":1,"payload_b64":"!!!not-base64!!!"}"#
    )
    .is_none());
}

#[test]
fn wifi_direct_config_serde_roundtrip() {
    let cfg = WifiP2pConfig {
        peer_address: "192.168.49.1".into(),
        port: 8080,
        is_group_owner: true,
        passphrase: Some("secret".into()),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: WifiP2pConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.peer_address, "192.168.49.1");
    assert_eq!(back.port, 8080);
    assert!(back.is_group_owner);
    assert_eq!(back.passphrase.as_deref(), Some("secret"));
}

#[test]
fn http3_client_constructors() {
    let a = Http3Client::new();
    let b = Http3Client::default();
    let c = Http3Client::with_socks_proxy(None);
    let d = Http3Client::with_socks_proxy(Some("127.0.0.1:9050".parse().unwrap()));
    drop((a, b, c, d));
}

#[tokio::test]
async fn http3_request_rejects_invalid_method_before_network() {
    let client = Http3Client::new();
    let e = client
        .request(
            "BAD METHOD",
            "https://example.com/",
            Default::default(),
            None,
        )
        .await
        .unwrap_err();
    assert!(e.contains("Invalid HTTP method"), "got {e}");
}

#[tokio::test]
async fn http3_request_unreachable_server_errors() {
    let client = Http3Client::new();
    let mut headers = std::collections::HashMap::new();
    headers.insert("x-test".to_string(), "1".to_string());
    let e = client
        .request("GET", "http://127.0.0.1:1/", headers, Some(vec![1, 2, 3]))
        .await
        .unwrap_err();
    assert!(
        e.contains("HTTP request failed") || e.contains("SSRF"),
        "got {e}"
    );
}

#[test]
fn http3_response_data_shape() {
    let resp = HttpResponseData {
        status: 200,
        headers: Default::default(),
        body: vec![1, 2, 3],
    };
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, vec![1, 2, 3]);
}
