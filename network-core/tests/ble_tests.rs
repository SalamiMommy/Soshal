//! BLE offline sync tests

use soshal_network_core::ble::{
    chunk_size, decrypt_envelope, device_name, encrypt_envelope, pubkey_fragment_from_device_name,
    reassemble_chunks, split_chunks, MAX_PAYLOAD_BYTES,
};
use soshal_pqc_core::{dsa, hybrid};

#[test]
fn device_name_roundtrip() {
    let pk = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let name = device_name(pk);
    assert_eq!(name, "SOSHAL_abcdef012345");
    assert_eq!(
        pubkey_fragment_from_device_name(&name).as_deref(),
        Some("abcdef012345")
    );
    assert_eq!(pubkey_fragment_from_device_name("SOSHAL_zz"), None);
    assert_eq!(pubkey_fragment_from_device_name("OTHER_abcdef012345"), None);
    assert_eq!(
        pubkey_fragment_from_device_name(name.replace('5', "z").as_str()),
        None
    );
}

#[test]
fn device_name_handles_short_pubkey() {
    let short_pk = "abc";
    let name = device_name(short_pk);
    assert_eq!(name, "SOSHAL_abc");
}

#[test]
fn device_name_handles_empty_pubkey() {
    let empty_pk = "";
    let name = device_name(empty_pk);
    assert_eq!(name, "SOSHAL_");
}

#[test]
fn chunk_roundtrip_single_and_multi() {
    for (data, mtu) in [
        ("hello".to_string(), 23),
        ("soshal sync payload".repeat(5), 23),
        ("unicode: 社会媒体 测试 🚀".to_string(), 512),
        ("x".repeat(100_000), 512),
    ] {
        let chunks = split_chunks(&data, mtu);
        assert!(!chunks.is_empty());
        assert_eq!(reassemble_chunks(&chunks), Some(data.to_string()));
    }
}

#[test]
fn chunk_size_caps() {
    assert_eq!(chunk_size(23), 20);
    assert_eq!(chunk_size(512), 509);
    assert_eq!(chunk_size(1000), 509);
    assert_eq!(chunk_size(10), 20);
}

#[test]
fn reassemble_rejects_gaps_dupes_and_mismatched_counts() {
    let chunks = split_chunks("abcdefghijklmnopqrstuvwxyz", 23);
    assert_eq!(reassemble_chunks(&chunks[1..]), None);
    assert_eq!(
        reassemble_chunks(&[chunks[0].clone(), chunks[0].clone()]),
        None
    );
    assert_eq!(reassemble_chunks(&[]), None);
    let mut swapped = chunks;
    let last = swapped.len() - 1;
    swapped.swap(0, last);
    assert_eq!(
        reassemble_chunks(&swapped),
        Some("abcdefghijklmnopqrstuvwxyz".to_string())
    );
}

#[test]
fn envelope_roundtrip_with_verification() {
    let (peer_pk, peer_sk) = hybrid::hybrid_keygen().unwrap();
    let (dsa_sk, dsa_pk) = dsa::dsa_keygen(None).unwrap();
    let payload = serde_json::json!({"posts": [1, 2, 3]}).to_string();
    let env = encrypt_envelope(&payload, &peer_pk, &dsa_sk, "npub_sender").unwrap();
    assert!(env.contains("\"pqc_ct\""));
    let out = decrypt_envelope(&env, &peer_sk, Some(&dsa_pk)).unwrap();
    assert_eq!(out, payload);
}

#[test]
fn envelope_rejects_wrong_sender_signature() {
    let (peer_pk, peer_sk) = hybrid::hybrid_keygen().unwrap();
    let (dsa_sk, _) = dsa::dsa_keygen(None).unwrap();
    let (_, other_pk) = dsa::dsa_keygen(None).unwrap();
    let payload = serde_json::json!({"test": true}).to_string();
    let env = encrypt_envelope(&payload, &peer_pk, &dsa_sk, "sender").unwrap();
    assert!(decrypt_envelope(&env, &peer_sk, Some(&other_pk)).is_err());
    let res = decrypt_envelope(&env, &peer_sk, None);
    assert!(res.is_ok(), "failed: {:?}", res);
}

#[test]
fn envelope_decrypts_with_wrong_sk_fails() {
    let (peer_pk, peer_sk) = hybrid::hybrid_keygen().unwrap();
    let (_, wrong_sk) = hybrid::hybrid_keygen().unwrap();
    let (dsa_sk, _) = dsa::dsa_keygen(None).unwrap();
    let payload = serde_json::json!({"test": true}).to_string();
    let env = encrypt_envelope(&payload, &peer_pk, &dsa_sk, "sender").unwrap();
    // Decryption with wrong secret key should fail
    assert!(decrypt_envelope(&env, &wrong_sk, None).is_err());
    // Decryption with correct secret key should succeed
    assert!(decrypt_envelope(&env, &peer_sk, None).is_ok());
}

#[test]
fn oversize_payload_rejected() {
    let (peer_pk, _) = hybrid::hybrid_keygen().unwrap();
    let (dsa_sk, _) = dsa::dsa_keygen(None).unwrap();
    let big = "x".repeat(MAX_PAYLOAD_BYTES + 1);
    assert!(encrypt_envelope(&big, &peer_pk, &dsa_sk, "sender").is_err());
}

#[test]
fn malformed_envelope_rejected() {
    let (_, my_sk) = hybrid::hybrid_keygen().unwrap();
    assert!(decrypt_envelope("not json", &my_sk, None).is_err());
    let evil = serde_json::json!({
        "pqc_ct": "ff".repeat(1121),
        "inner": "x",
        "senderPubkey": "s",
        "dsaSig": "y",
    });
    assert!(decrypt_envelope(&evil.to_string(), &my_sk, None).is_err());
    assert!(decrypt_envelope(&evil.to_string(), &my_sk, Some("00")).is_err());
}
