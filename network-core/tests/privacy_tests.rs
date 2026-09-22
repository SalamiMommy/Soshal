//! Privacy level utilities tests

use soshal_network_core::privacy::{
    describe_privacy_level, describe_privacy_level_json, get_max_wot_distance,
    get_max_wot_distance_json, select_relays, select_relays_json, DescribeLevelInput,
    SelectRelaysInput,
};

#[test]
fn describe_privacy_level_public() {
    let result = describe_privacy_level("public");
    assert_eq!(result.label, "Public");
    assert_eq!(result.relay_count, 4);
    assert_eq!(result.feed_reach, "Global Nostr Network");
}

#[test]
fn describe_privacy_level_only_me() {
    let result = describe_privacy_level("only_me");
    assert_eq!(result.label, "Only Me");
    assert_eq!(result.relay_count, 0);
    assert_eq!(result.feed_reach, "Local Only");
}

#[test]
fn describe_privacy_level_friends() {
    let result = describe_privacy_level("friends");
    assert_eq!(result.label, "Friends");
    assert_eq!(result.relay_count, 2);
    assert_eq!(result.feed_reach, "Direct Connections");
}

#[test]
fn describe_privacy_level_network() {
    let result = describe_privacy_level("network");
    assert_eq!(result.label, "Network");
    assert_eq!(result.relay_count, 3);
    assert_eq!(result.feed_reach, "Extended Network");
}

#[test]
fn describe_privacy_level_unknown_defaults_to_network() {
    let result = describe_privacy_level("unknown");
    assert_eq!(result.label, "Network");
}

#[test]
fn describe_privacy_level_json_interface() {
    let input = serde_json::to_string(&DescribeLevelInput {
        level: "public".to_string(),
    })
    .unwrap();
    let result = describe_privacy_level_json(&input);
    assert!(result.contains("Public"));
}

#[test]
fn describe_privacy_level_json_plain_string() {
    let result = describe_privacy_level_json("friends");
    assert!(result.contains("Friends"));
}

#[test]
fn get_max_wot_distance_public() {
    assert_eq!(get_max_wot_distance("public"), 2);
}

#[test]
fn get_max_wot_distance_network() {
    assert_eq!(get_max_wot_distance("network"), 2);
}

#[test]
fn get_max_wot_distance_friends() {
    assert_eq!(get_max_wot_distance("friends"), 1);
}

#[test]
fn get_max_wot_distance_only_me() {
    assert_eq!(get_max_wot_distance("only_me"), 0);
}

#[test]
fn get_max_wot_distance_unknown_defaults_to_2() {
    assert_eq!(get_max_wot_distance("unknown"), 2);
}

#[test]
fn get_max_wot_distance_json_interface() {
    let input = r#"{"level": "friends"}"#;
    assert_eq!(get_max_wot_distance_json(input), 1);
}

#[test]
fn select_relays_public_with_i2p_no_freenet() {
    let relays = select_relays("public", true, false, None, None);
    assert_eq!(relays.len(), 7);
    let i2p_count = relays.iter().filter(|r| r.contains("i2p")).count();
    assert_eq!(i2p_count, 3);
    assert!(relays[0].contains("i2p"));
}

#[test]
fn select_relays_network_with_i2p_no_freenet() {
    let relays = select_relays("network", true, false, None, None);
    assert_eq!(relays.len(), 5);
}

#[test]
fn select_relays_friends_with_freenet_no_i2p() {
    let relays = select_relays("friends", false, true, None, None);
    assert_eq!(relays.len(), 4);
    let freenet_count = relays.iter().filter(|r| r.contains(".free")).count();
    assert_eq!(freenet_count, 2);
}

#[test]
fn select_relays_only_me() {
    let relays = select_relays("only_me", false, false, None, None);
    assert!(relays.is_empty(), "only_me routes mesh-only, no relays");
    let with_public = select_relays("only_me", false, false, None, None);
    assert!(
        with_public.iter().all(|r| !r.contains("relay.damus.io")),
        "no public relay URL for only_me"
    );
}

#[test]
fn select_relays_friends_drops_public_when_distance_exceeds() {
    // L6: friends audience whose graph distance exceeds the friends max (1)
    // must not select public relays (strangers).
    let kept = select_relays("friends", false, false, None, None);
    assert!(kept.iter().any(|r| r.contains("relay.damus.io")));
    let dropped = select_relays("friends", false, false, None, Some(2));
    assert!(
        dropped.is_empty(),
        "friends + distance>1 -> no public relays"
    );
    // Public/network levels unaffected by distance.
    let public = select_relays("public", false, false, None, Some(2));
    assert!(public.contains(&"wss://relay.damus.io".to_string()));
}

#[test]
fn select_relays_with_pubkey_adds_hosted() {
    let relays = select_relays(
        "public",
        true,
        true,
        Some("npub1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"),
        Some(1),
    );
    // The function adds hosted relays based on pubkey prefix
    // Check that i2p and freenet relays are present
    assert!(relays.iter().any(|r| r.contains("i2p")));
    assert!(relays.iter().any(|r| r.contains(".free")));
}

#[test]
fn select_relays_with_pubkey_wot_distance_too_high() {
    let relays = select_relays(
        "friends",
        true,
        true,
        Some("npub1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"),
        Some(2),
    );
    // Should not add hosted relays since WoT distance exceeds friends level
    assert!(!relays.iter().any(|r| r.contains("npub1234567890abcde")));
}

#[test]
fn select_relays_json_interface() {
    let input = serde_json::to_string(&SelectRelaysInput {
        level: "public".to_string(),
        i2p_available: true,
        freenet_available: false,
        pubkey: None,
        wot_distance: None,
    })
    .unwrap();
    let result = select_relays_json(&input);
    assert!(result.contains("i2p"));
}

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use soshal_network_core::lan::is_private_ip;

#[test]
fn ip_privacy_matrix() {
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::new(172, 31, 255, 255))));
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(is_private_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    assert!(is_private_ip(IpAddr::V6(Ipv6Addr::new(
        0xfe80, 0, 0, 0, 0, 0, 0, 1
    ))));
    assert!(is_private_ip(IpAddr::V6(Ipv6Addr::new(
        0xfc00, 0, 0, 0, 0, 0, 0, 1
    ))));
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    assert!(is_private_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 0, 1))));

    assert!(!is_private_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    assert!(!is_private_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    assert!(!is_private_ip(IpAddr::V4(Ipv4Addr::new(172, 32, 0, 1))));
    assert!(!is_private_ip(IpAddr::V4(Ipv4Addr::new(192, 169, 0, 1))));
    assert!(!is_private_ip("2606:4700::1111".parse().unwrap()));
    assert!(!is_private_ip("2001:4860:4860::8888".parse().unwrap()));

    assert!(is_private_ip("::ffff:10.0.0.1".parse().unwrap()));
    assert!(is_private_ip("::ffff:172.16.0.1".parse().unwrap()));
    assert!(is_private_ip("::ffff:192.168.1.1".parse().unwrap()));
    assert!(
        !is_private_ip("::ffff:8.8.8.8".parse().unwrap()),
        "public v4 embedded in mapped v6 stays public"
    );
}
