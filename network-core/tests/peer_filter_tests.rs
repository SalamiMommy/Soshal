//! Peer filter tests

use soshal_network_core::peer_filter::dedup_server_list_json;

#[test]
fn dedup_server_list_basic() {
    let input = r#"["wss://relay1.com", "wss://relay2.com", "wss://relay1.com"]"#;
    let result = dedup_server_list_json(input);
    let parsed: Vec<String> = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed.len(), 2);
    assert!(parsed.contains(&"wss://relay1.com".to_string()));
    assert!(parsed.contains(&"wss://relay2.com".to_string()));
}

#[test]
fn dedup_server_list_all_duplicates() {
    let input = r#"["wss://relay1.com", "wss://relay1.com", "wss://relay1.com"]"#;
    let result = dedup_server_list_json(input);
    let parsed: Vec<String> = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0], "wss://relay1.com");
}

#[test]
fn dedup_server_list_empty() {
    let input = r#"[]"#;
    let result = dedup_server_list_json(input);
    assert_eq!(result, "[]");
}

#[test]
fn dedup_server_list_invalid_json() {
    let input = "invalid json";
    let result = dedup_server_list_json(input);
    assert_eq!(result, "[]");
}

#[test]
fn dedup_server_list_preserves_order() {
    let input =
        r#"["wss://relay3.com", "wss://relay1.com", "wss://relay2.com", "wss://relay1.com"]"#;
    let result = dedup_server_list_json(input);
    let parsed: Vec<String> = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed,
        vec!["wss://relay3.com", "wss://relay1.com", "wss://relay2.com"]
    );
}

#[test]
fn dedup_server_list_case_sensitive() {
    let input = r#"["wss://relay1.com", "WSS://RELAY1.COM", "wss://Relay1.com"]"#;
    let result = dedup_server_list_json(input);
    let parsed: Vec<String> = serde_json::from_str(&result).unwrap();
    // All three should be preserved since they're different strings
    assert_eq!(parsed.len(), 3);
}
