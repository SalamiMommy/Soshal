//! LAN peer discovery tests

use soshal_network_core::lan::{beacon_body, beacon_mac, is_private_ip, parse_beacon, sync_token};

#[test]
fn is_private_ip_recognizes_rfc1918() {
    assert!(is_private_ip("10.0.0.1".parse().unwrap()));
    assert!(is_private_ip("172.16.0.1".parse().unwrap()));
    assert!(is_private_ip("172.31.255.255".parse().unwrap()));
    assert!(is_private_ip("192.168.1.1".parse().unwrap()));
    assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
    assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
}

#[test]
fn is_private_ip_recognizes_loopback() {
    assert!(is_private_ip("127.0.0.1".parse().unwrap()));
    assert!(is_private_ip("127.0.0.255".parse().unwrap()));
}

#[test]
fn is_private_ip_recognizes_link_local() {
    assert!(is_private_ip("169.254.1.1".parse().unwrap()));
}

#[test]
fn is_private_ip_recognizes_ipv6_private() {
    assert!(is_private_ip("fc00::1".parse().unwrap()));
    assert!(is_private_ip("fe80::1".parse().unwrap()));
    assert!(is_private_ip("::1".parse().unwrap()));
    assert!(!is_private_ip("2001:4860:4860::8888".parse().unwrap()));
}

#[test]
fn beacon_body_formats_correctly() {
    let body = beacon_body("MAGIC", "npub123", 8080);
    assert_eq!(body, "MAGIC:npub123:8080");
}

#[test]
fn beacon_mac_is_deterministic() {
    let key = [0u8; 32];
    let body = "MAGIC:npub123:8080";
    let mac1 = beacon_mac(&key, body);
    let mac2 = beacon_mac(&key, body);
    assert_eq!(mac1, mac2);
}

#[test]
fn beacon_mac_differs_for_different_keys() {
    let key1 = [0u8; 32];
    let mut key2 = [0u8; 32];
    key2[0] = 1;
    let body = "MAGIC:npub123:8080";
    assert_ne!(beacon_mac(&key1, body), beacon_mac(&key2, body));
}

#[test]
fn parse_beacon_valid() {
    let key = [0u8; 32];
    let magic = "MAGIC";
    let body = beacon_body(
        magic,
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        8080,
    );
    let mac = beacon_mac(&key, &body);
    let beacon = format!("{}:{}", body, mac);

    let result = parse_beacon(&key, magic, &beacon, 9000);
    assert!(result.is_some());
    let (pubkey, port) = result.unwrap();
    assert_eq!(
        pubkey,
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
    );
    assert_eq!(port, 8080);
}

#[test]
fn parse_beacon_rejects_wrong_magic() {
    let key = [0u8; 32];
    let magic = "MAGIC";
    let body = beacon_body(
        "WRONG",
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        8080,
    );
    let mac = beacon_mac(&key, &body);
    let beacon = format!("{}:{}", body, mac);

    let result = parse_beacon(&key, magic, &beacon, 9000);
    assert!(result.is_none());
}

#[test]
fn parse_beacon_rejects_bad_mac() {
    let key = [0u8; 32];
    let magic = "MAGIC";
    let body = beacon_body(
        magic,
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        8080,
    );
    let beacon = format!("{}:badmac", body);

    let result = parse_beacon(&key, magic, &beacon, 9000);
    assert!(result.is_none());
}

#[test]
fn parse_beacon_rejects_invalid_pubkey() {
    let key = [0u8; 32];
    let magic = "MAGIC";
    let body = beacon_body(magic, "not64chars", 8080);
    let mac = beacon_mac(&key, &body);
    let beacon = format!("{}:{}", body, mac);

    let result = parse_beacon(&key, magic, &beacon, 9000);
    assert!(result.is_none());
}

#[test]
fn parse_beacon_uses_default_port_on_parse_failure() {
    let key = [0u8; 32];
    let magic = "MAGIC";
    let body = format!(
        "{}:{}:{}",
        magic, "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789", "invalid"
    );
    let mac = beacon_mac(&key, &body);
    let beacon = format!("{}:{}", body, mac);

    let result = parse_beacon(&key, magic, &beacon, 9000);
    assert!(result.is_some());
    let (_, port) = result.unwrap();
    assert_eq!(port, 9000);
}

#[test]
fn sync_token_is_deterministic() {
    let key = [0u8; 32];
    let token1 = sync_token(&key);
    let token2 = sync_token(&key);
    assert_eq!(token1, token2);
}

#[test]
fn sync_token_is_16_bytes_hex() {
    let key = [0u8; 32];
    let token = sync_token(&key);
    assert_eq!(token.len(), 32); // 16 bytes = 32 hex chars
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn sync_token_differs_for_different_keys() {
    let key1 = [0u8; 32];
    let mut key2 = [0u8; 32];
    key2[0] = 1;
    let token1 = sync_token(&key1);
    let token2 = sync_token(&key2);
    assert_ne!(token1, token2);
}
