//! Integration tests for soshal-messaging-core: NIP-44 message wrapping and
//! giftwrap envelope builders.

use soshal_messaging_core::giftwrap::{build_rumor_envelope_json, build_seal_envelope_json};
use soshal_messaging_core::nip44wrap::{unwrap_message, wrap_message};

fn key(byte: u8) -> [u8; 32] {
    [byte; 32]
}

#[test]
fn wrap_unwrap_roundtrip() {
    let plaintext = b"secret message payload";
    let wrapped = wrap_message(plaintext, &key(0x42)).unwrap();
    assert!(!wrapped.ciphertext.is_empty());
    assert!(wrapped.conversation_pubkey.is_none());

    let unwrapped = unwrap_message(&wrapped.ciphertext, &key(0x42)).unwrap();
    assert_eq!(unwrapped, plaintext);
}

#[test]
fn wrap_rejects_empty_plaintext() {
    assert!(wrap_message(b"", &key(0x42)).is_err());
}

#[test]
fn unwrap_rejects_wrong_key() {
    let wrapped = wrap_message(b"hello", &key(0x42)).unwrap();
    assert!(unwrap_message(&wrapped.ciphertext, &key(0x00)).is_err());
}

#[test]
fn unwrap_rejects_tampered_ciphertext() {
    let wrapped = wrap_message(b"hello", &key(0x42)).unwrap();
    let mut ct = wrapped.ciphertext.clone();
    let last = ct.pop().unwrap();
    ct.push(if last == 'A' { 'B' } else { 'A' });
    assert!(unwrap_message(&ct, &key(0x42)).is_err());
}

#[test]
fn unwrap_rejects_garbage() {
    assert!(unwrap_message("not-base64!!!", &key(0x42)).is_err());
    assert!(unwrap_message("", &key(0x42)).is_err());
}

#[test]
fn rumor_envelope_json_roundtrip() {
    let input = r#"{"pqcCt":"abcd1234","rumor":"{\"content\":\"hi\"}"}"#;
    let out = build_rumor_envelope_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["pqc_ct"], "abcd1234");
    assert_eq!(v["rumor"], r#"{"content":"hi"}"#);
}

#[test]
fn rumor_envelope_json_empty_on_malformed_input() {
    assert_eq!(build_rumor_envelope_json("not json"), "");
    assert_eq!(build_rumor_envelope_json(r#"{"pqcCt":1}"#), "");
}

#[test]
fn seal_envelope_json_roundtrip_with_peer_pk() {
    let input =
        r#"{"pqcCt":"ct123","rumorJson":"{}","dsaPublicKey":"pk1","peerDsaPublicKey":"pk2"}"#;
    let out = build_seal_envelope_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["pqc_ct"], "ct123");
    assert_eq!(v["rumor"], "{}");
    assert_eq!(v["pqc_pk"], "pk1");
    assert_eq!(v["pqc_pk_peer"], "pk2");
}

#[test]
fn seal_envelope_json_omits_peer_pk_when_absent() {
    let input = r#"{"pqcCt":"ct123","rumorJson":"{}","dsaPublicKey":"pk1"}"#;
    let out = build_seal_envelope_json(input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("pqc_pk_peer").is_none());
}

#[test]
fn seal_envelope_json_empty_on_malformed_input() {
    assert_eq!(build_seal_envelope_json("garbage"), "");
}

#[test]
fn wrap_unwrap_binary_and_large_payload() {
    let binary_data: Vec<u8> = (0..=255).collect();
    let wrapped = wrap_message(&binary_data, &key(0x7F)).unwrap();
    let unwrapped = unwrap_message(&wrapped.ciphertext, &key(0x7F)).unwrap();
    assert_eq!(unwrapped, binary_data);

    let large_payload = vec![0xAB; 10000];
    let wrapped_large = wrap_message(&large_payload, &key(0x7F)).unwrap();
    let unwrapped_large = unwrap_message(&wrapped_large.ciphertext, &key(0x7F)).unwrap();
    assert_eq!(unwrapped_large, large_payload);
}

#[test]
fn encrypted_message_struct_serde() {
    let wrapped = wrap_message(b"test serialization", &key(0x12)).unwrap();
    let json_str = serde_json::to_string(&wrapped).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.get("ciphertext").is_some());
    assert!(parsed.get("conversation_pubkey").is_some());
}

#[test]
fn rumor_envelope_json_edge_cases() {
    // Missing pqcCt
    let input1 = r#"{"rumor":"test"}"#;
    assert_eq!(build_rumor_envelope_json(input1), "");

    // Missing rumor
    let input2 = r#"{"pqcCt":"abcd"}"#;
    assert_eq!(build_rumor_envelope_json(input2), "");
}

#[test]
fn seal_envelope_json_edge_cases() {
    // Missing dsaPublicKey
    let input1 = r#"{"pqcCt":"ct","rumorJson":"{}"}"#;
    assert_eq!(build_seal_envelope_json(input1), "");

    // Invalid dsaPublicKey type
    let input2 = r#"{"pqcCt":"ct","rumorJson":"{}","dsaPublicKey":123}"#;
    assert_eq!(build_seal_envelope_json(input2), "");
}
