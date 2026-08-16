//! Integration tests for soshal-storage-core.

use soshal_storage_core::erasure_fountain::{decode_fountain, encode_fountain};
use soshal_storage_core::util::{fail_json, hex_to_32_bytes};

// ---------------------------------------------------------------------------
// Util
// ---------------------------------------------------------------------------

#[test]
fn fail_json_builds_error_envelope() {
    let out = fail_json("ciphertext_json", "boom");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["error"], "boom");
    assert!(v["ciphertext_json"].is_null());
}

#[test]
fn hex_to_32_bytes_validation() {
    let ok = hex_to_32_bytes(&"ab".repeat(32)).unwrap();
    assert_eq!(ok.len(), 32);
    assert!(hex_to_32_bytes(&"ab".repeat(31)).is_err());
    assert!(hex_to_32_bytes(&"ab".repeat(33)).is_err());
    assert!(hex_to_32_bytes("zz").is_err());
    assert!(hex_to_32_bytes("").is_err());
}

// ---------------------------------------------------------------------------
// Erasure fountain
// ---------------------------------------------------------------------------

fn fountain_payload() -> Vec<u8> {
    (0..25_600u32).map(|i| (i % 251) as u8).collect()
}

#[test]
fn erasure_fountain_drop_30_percent_still_reconstructs() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let dropped = (encoded.packets.len() as f32 * 0.3).ceil() as usize;
    let available = encoded.packets[..encoded.packets.len() - dropped].to_vec();
    let decoded = decode_fountain(&encoded.manifest, &available).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn erasure_fountain_any_k_packets_reconstruct() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let k = encoded.manifest.num_source_symbols as usize;
    let subset = encoded.packets[..k].to_vec();
    let decoded = decode_fountain(&encoded.manifest, &subset).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn erasure_fountain_insufficient_packets_errors() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let few = encoded.packets[..encoded.manifest.num_source_symbols as usize / 2].to_vec();
    let err = decode_fountain(&encoded.manifest, &few).unwrap_err();
    assert!(err.contains("Insufficient"));
}

#[test]
fn erasure_fountain_empty_payload_rejected() {
    assert!(encode_fountain(&[], 0.5).is_err());
}

#[test]
fn erasure_fountain_corrupted_packet_not_detected() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let mut packets = encoded.packets.clone();
    packets[0][4 + 17] ^= 0xFF;
    let decoded = decode_fountain(&encoded.manifest, &packets).unwrap();
    assert_ne!(decoded, original);
}
