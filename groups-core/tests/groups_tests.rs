//! Integration tests for soshal-groups-core: NIP-29 membership checks,
//! group-message sealing, key distribution content and event parsing.

use soshal_groups_core::group::channels::parse_group_channels_json;
use soshal_groups_core::group::chat::parse_group_chat_messages_json;
use soshal_groups_core::group::groups::parse_groups_json;
use soshal_groups_core::group::join_requests::parse_group_join_requests_json;
use soshal_groups_core::group::posts::parse_group_posts_json;
use soshal_groups_core::group_enc::envelope::{
    build_group_message_envelope_json, validate_group_permissions, validate_group_permissions_json,
    GroupPermissionInput,
};
use soshal_groups_core::group_enc::key_distribution::{
    build_key_distribution_content, parse_key_distribution_content, BuildKeyDistInput,
};
use soshal_groups_core::group_enc::seal::{
    group_message_envelope, nip44_open_group, nip44_seal_group,
};
use soshal_groups_core::membership::{can_send_to_channel, is_admin};

#[test]
fn membership_role_checks() {
    assert!(can_send_to_channel(3, 3));
    assert!(can_send_to_channel(5, 3));
    assert!(!can_send_to_channel(2, 3));
    assert!(can_send_to_channel(0, 0));
    assert!(is_admin(3));
    assert!(is_admin(5));
    assert!(!is_admin(2));
    assert!(!is_admin(0));
}

#[test]
fn group_seal_roundtrip() {
    let key_hex = hex::encode([0x42u8; 32]);
    let ct = nip44_seal_group("hello group", &key_hex).unwrap();
    assert_eq!(nip44_open_group(&ct, &key_hex).unwrap(), "hello group");
    assert!(nip44_seal_group("x", "zz").is_err());
    assert!(nip44_seal_group("x", &hex::encode([0u8; 31])).is_err());
    assert!(nip44_open_group("garbage", &key_hex).is_err());
    let wrong = hex::encode([0x00u8; 32]);
    assert!(nip44_open_group(&ct, &wrong).is_err());
}

#[test]
fn group_envelope_plain_and_sealed() {
    let env = group_message_envelope("open group text", None).unwrap();
    assert_eq!(env, "open group text");
    let key_hex = hex::encode([0x07u8; 32]);
    let sealed = group_message_envelope("secret", Some(&key_hex)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&sealed).unwrap();
    assert_eq!(v["v"], 1);
    let payload = v["payload"].as_str().unwrap();
    assert_eq!(nip44_open_group(payload, &key_hex).unwrap(), "secret");
    let bad = group_message_envelope("secret", Some("zz"));
    assert!(bad.is_err());
}

#[test]
fn message_envelope_json() {
    let input = r#"{"pqcCt":"ct1","payload":"payload1"}"#;
    let out = build_group_message_envelope_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["pqc_ct"], "ct1");
    assert_eq!(v["payload"], "payload1");
    assert_eq!(build_group_message_envelope_json("garbage"), "{}");
}

#[test]
fn group_permissions() {
    let owner = GroupPermissionInput {
        role: "owner".into(),
        permissions_mask: 0,
        required_bit: 4,
    };
    assert!(validate_group_permissions(&owner).allowed);
    let admin = GroupPermissionInput {
        role: "admin".into(),
        permissions_mask: 0,
        required_bit: 4,
    };
    assert!(validate_group_permissions(&admin).allowed);
    let member_ok = GroupPermissionInput {
        role: "member".into(),
        permissions_mask: 5,
        required_bit: 4,
    };
    assert!(validate_group_permissions(&member_ok).allowed);
    let member_denied = GroupPermissionInput {
        role: "member".into(),
        permissions_mask: 2,
        required_bit: 4,
    };
    assert!(!validate_group_permissions(&member_denied).allowed);
    let bit0 = GroupPermissionInput {
        role: "member".into(),
        permissions_mask: 0,
        required_bit: 0,
    };
    assert!(validate_group_permissions(&bit0).allowed);
}

#[test]
fn group_permissions_json() {
    let out = validate_group_permissions_json(
        r#"{"role":"member","permissions_mask":5,"required_bit":4}"#,
    );
    assert!(out.contains("\"allowed\":true"));
    let out2 = validate_group_permissions_json(
        r#"{"role":"member","permissions_mask":1,"required_bit":4}"#,
    );
    assert!(out2.contains("\"allowed\":false"));
    assert_eq!(
        validate_group_permissions_json("garbage"),
        r#"{"allowed":false}"#
    );
    assert!(validate_group_permissions_json(r#"{"required_bit":2}"#).contains("\"allowed\":false"));
}

#[test]
fn key_distribution_never_leaks_shared_key() {
    let input = BuildKeyDistInput {
        group_id: "g1".into(),
        shared_key: "TOP_SECRET_GROUP_KEY".into(),
        shared_pubkey: "pk1".into(),
        pqc_ct: "ct1".into(),
    };
    let out = build_key_distribution_content(&input);
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["groupId"], "g1");
    assert_eq!(json["pqc_ct"], "ct1");
    assert!(json.get("sharedKey").is_none());
    assert!(!json.to_string().contains("TOP_SECRET_GROUP_KEY"));
}

#[test]
fn parse_key_distribution_valid() {
    let content =
        r#"{"groupId":"g1","sharedKey":"LEGACY_PLAINTEXT","sharedPubkey":"pk9","pqcCt":"ct9"}"#;
    let parsed = parse_key_distribution_content(content);
    assert!(parsed.valid);
    assert_eq!(parsed.group_id, "g1");
    assert_eq!(parsed.pqc_ct, "ct9");
    assert!(parsed.error_reason.is_none());
    let serialized = serde_json::to_string(&parsed).unwrap();
    assert!(!serialized.contains("LEGACY_PLAINTEXT"));
}

#[test]
fn parse_key_distribution_invalid() {
    let missing = parse_key_distribution_content(r#"{"pqcCt":"ct1"}"#);
    assert!(!missing.valid);
    assert_eq!(missing.error_reason.as_deref(), Some("Missing groupId"));
    let no_ct = parse_key_distribution_content(r#"{"groupId":"g1"}"#);
    assert!(!no_ct.valid);
    assert_eq!(no_ct.error_reason.as_deref(), Some("Missing pqc_ct"));
    let garbage = parse_key_distribution_content("not json");
    assert!(!garbage.valid);
    assert!(garbage.error_reason.is_some());
}

#[test]
fn parse_groups_json_roundtrip() {
    let input = serde_json::json!({
        "self_pubkey": "me",
        "events": [{
            "id": "e1", "pubkey": "me",
            "kind": 39000, "created_at": 1700000000.0,
            "content": "{\"about\":\"a group\",\"picture\":\"https://x/y.png\",\"audience\":\"private\"}",
            "tags": [["d","group-1"],["name","My Group"],["audience","private"]]
        }]
    })
    .to_string();
    let out = parse_groups_json(&input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["id"], "group-1");
    assert_eq!(v[0]["name"], "My Group");
    assert_eq!(v[0]["is_owner"], true);
    assert_eq!(v[0]["about"], "a group");
    assert_eq!(v[0]["created_at"], 1_700_000_000_000.0);
}

#[test]
fn parse_groups_json_skips_invalid() {
    let input = serde_json::json!({
        "self_pubkey": "me",
        "events": [{
            "id": "e1", "pubkey": "other", "kind": 39000, "created_at": 1.0,
            "content": "not json", "tags": [["name","NoId"]]
        }]
    })
    .to_string();
    assert_eq!(parse_groups_json(&input), "[]");
    assert_eq!(parse_groups_json("garbage"), "[]");
}

#[test]
fn parse_group_channels_json_parses() {
    let input = serde_json::json!({
        "events": [{
            "id": "e1", "pubkey": "pk", "kind": 39003, "created_at": 5.0,
            "content": "", "tags": [["d","chan-1"],["g","group-1"],["name","General"],["category","chat"]]
        }]
    })
    .to_string();
    let out = soshal_groups_core::group::channels::parse_group_channels_json(&input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["id"], "chan-1");
    assert_eq!(v[0]["group_id"], "group-1");
    assert_eq!(v[0]["category"], "chat");
    assert_eq!(parse_group_channels_json("garbage"), "[]");
}

#[test]
fn parse_group_chat_messages_json_parses() {
    let input = serde_json::json!({
        "group_id": "grp-x",
        "events": [{
            "id": "e1", "pubkey": "pk", "kind": 9, "created_at": 5.0,
            "content": "hello", "tags": [["d","grp-x"],["e","parent-id"]]
        }]
    })
    .to_string();
    let out = soshal_groups_core::group::chat::parse_group_chat_messages_json(&input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["content"], "hello");
    assert_eq!(parse_group_chat_messages_json("garbage"), "[]");
}

#[test]
fn parse_group_join_requests_json_parses() {
    let input = serde_json::json!({
        "events": [{
            "id": "e1", "pubkey": "requester", "kind": 9021, "created_at": 5.0,
            "content": "", "tags": [["request","join"],["p","requester"]]
        }]
    })
    .to_string();
    let out = soshal_groups_core::group::join_requests::parse_group_join_requests_json(&input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["pubkey"], "requester");
    let forged = serde_json::json!({
        "events": [{
            "id": "e2", "pubkey": "attacker", "kind": 9021, "created_at": 5.0,
            "content": "", "tags": [["request","join"],["p","victim"]]
        }]
    })
    .to_string();
    let out2 = soshal_groups_core::group::join_requests::parse_group_join_requests_json(&forged);
    let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
    assert_eq!(v2.as_array().unwrap().len(), 0);
    assert_eq!(parse_group_join_requests_json("garbage"), "[]");
}

#[test]
fn parse_group_posts_json_parses() {
    let input = serde_json::json!({
        "self_pubkey": "me",
        "group_id": "grp-p",
        "events": [{
            "id": "e1", "pubkey": "me", "kind": 20, "created_at": 5.0,
            "content": "check this out",
            "tags": [["d","grp-p"],["h","chan-1"],["image","https://x/1.png"]]
        }]
    })
    .to_string();
    let out = soshal_groups_core::group::posts::parse_group_posts_json(&input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["content"], "check this out");
    assert_eq!(parse_group_posts_json("garbage"), "[]");
}

#[test]
fn test_key_distribution_parsing_edge_cases() {
    let missing_pubkey = parse_key_distribution_content(r#"{"groupId":"g1","pqcCt":"ct1"}"#);
    assert!(missing_pubkey.valid); // sharedPubkey is optional
    assert_eq!(missing_pubkey.group_id, "g1");

    let invalid_json = parse_key_distribution_content(r#"{"groupId":123}"#);
    assert!(!invalid_json.valid);
}

#[test]
fn test_access_community_password_hashing_and_verification() {
    use soshal_groups_core::access::{
        hash_community_password, is_community_private, verify_community_password,
    };

    let pass = "ClubSecretPassword!42";
    let stored = hash_community_password(pass).expect("valid hash");
    assert!(verify_community_password(pass, &stored));
    assert!(!verify_community_password("wrongPass", &stored));
    assert!(!verify_community_password("", &stored));

    assert!(is_community_private("private", false));
    assert!(is_community_private("open", true));
    assert!(!is_community_private("open", false));
}
