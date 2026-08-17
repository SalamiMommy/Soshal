//! Integration tests for soshal-storage-core.

use ring::aead::NONCE_LEN;
use soshal_storage_core::crypto_blob::{decrypt_blob_at_rest, encrypt_blob_at_rest};
use soshal_storage_core::erasure_fountain::{decode_fountain, encode_fountain};
use soshal_storage_core::io_uring_backend::{IoEngineMode, IoUringEngine};
use soshal_storage_core::util::{fail_json, hex_to_32_bytes};
use soshal_storage_core::{json_in, json_out};

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

// ---------------------------------------------------------------------------
// JSON envelope re-exports (lib.rs)
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
struct EnvelopeProbe {
    a: u32,
    b: String,
}

#[test]
fn json_out_json_in_roundtrip() {
    let probe = EnvelopeProbe {
        a: 7,
        b: "payload".into(),
    };
    let out = json_out(&probe, "{}");
    let back: EnvelopeProbe = json_in(&out, EnvelopeProbe::default_probe());
    assert_eq!(back, probe);
}

#[test]
fn json_in_malformed_uses_fallback() {
    let fallback = EnvelopeProbe::default_probe();
    assert_eq!(json_in("not json", fallback.clone()), fallback);
}

impl EnvelopeProbe {
    fn default_probe() -> Self {
        Self {
            a: 0,
            b: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Crypto blob at rest (crypto_blob.rs)
// ---------------------------------------------------------------------------

#[test]
fn crypto_blob_roundtrip() {
    let key = [7u8; 32];
    let msg = b"Soshal at-rest cached media blob 1234567890";
    let enc = encrypt_blob_at_rest(&key, msg).unwrap();
    assert_ne!(enc, msg);
    assert_eq!(enc.len(), NONCE_LEN + msg.len() + 16);
    let dec = decrypt_blob_at_rest(&key, &enc).unwrap();
    assert_eq!(&dec[..], msg);
}

#[test]
fn crypto_blob_roundtrip_empty_payload() {
    let key = [9u8; 32];
    let enc = encrypt_blob_at_rest(&key, b"").unwrap();
    let dec = decrypt_blob_at_rest(&key, &enc).unwrap();
    assert!(dec.is_empty());
}

#[test]
fn crypto_blob_wrong_key_fails() {
    let key = [1u8; 32];
    let enc = encrypt_blob_at_rest(&key, b"data").unwrap();
    let other = [2u8; 32];
    assert!(decrypt_blob_at_rest(&other, &enc).is_err());
}

#[test]
fn crypto_blob_corrupted_ciphertext_fails() {
    let key = [3u8; 32];
    let mut enc = encrypt_blob_at_rest(&key, b"tamper me").unwrap();
    let last = enc.len() - 1;
    enc[last] ^= 0xFF;
    assert!(decrypt_blob_at_rest(&key, &enc).is_err());
}

#[test]
fn crypto_blob_short_payload_fails() {
    let key = [4u8; 32];
    assert!(decrypt_blob_at_rest(&key, &[0u8; 8]).is_err());
}

#[test]
fn crypto_blob_nonce_unique_per_encryption() {
    let key = [5u8; 32];
    let a = encrypt_blob_at_rest(&key, b"same").unwrap();
    let b = encrypt_blob_at_rest(&key, b"same").unwrap();
    assert_ne!(a, b);
}

// ---------------------------------------------------------------------------
// io_uring backend (io_uring_backend.rs)
// ---------------------------------------------------------------------------

fn temp_path(scope: &str, tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "soshal_storage_it_{}_{}_{}",
        std::process::id(),
        scope,
        tag
    ))
}

#[test]
fn io_uring_write_read_roundtrip_all_modes() {
    let payload: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
    for mode in [
        IoEngineMode::IoUringKernelRing,
        IoEngineMode::MemmapZeroCopy,
        IoEngineMode::StandardTokioFs,
    ] {
        let path = temp_path("write_read", &format!("{mode:?}"));
        let engine = IoUringEngine { mode };
        engine.write_chunk(&path, &payload).unwrap();
        let read_back = engine.read_chunk(&path).unwrap();
        assert_eq!(read_back, payload);
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn io_uring_mmap_matches_write() {
    let engine = IoUringEngine {
        mode: IoEngineMode::MemmapZeroCopy,
    };
    let path = temp_path("mmap", "a");
    let payload: Vec<u8> = (0..64 * 1024).map(|i| (i % 97) as u8).collect();
    engine.write_chunk(&path, &payload).unwrap();
    let map = engine.mmap_chunk(&path).unwrap();
    assert_eq!(map.as_ref(), payload.as_slice());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn io_uring_new_and_default_agree() {
    let engine = IoUringEngine::new();
    assert_eq!(engine.mode, IoUringEngine::default().mode);
}

#[test]
fn io_uring_missing_file_read_errors() {
    let engine = IoUringEngine::new();
    let missing = temp_path("missing", "nope");
    let _ = std::fs::remove_file(&missing);
    assert!(engine.read_chunk(&missing).is_err());
    assert!(engine.mmap_chunk(&missing).is_err());
}

#[test]
fn io_uring_write_bad_path_errors() {
    let engine = IoUringEngine::new();
    let bad = std::env::temp_dir()
        .join(format!(
            "soshal_storage_it_{}_no_such_dir",
            std::process::id()
        ))
        .join("file.bin");
    assert!(engine.write_chunk(&bad, b"x").is_err());
}

#[test]
fn io_uring_empty_file_reads_empty() {
    let path = temp_path("empty", "a");
    std::fs::File::create(&path).unwrap();
    for mode in [
        IoEngineMode::IoUringKernelRing,
        IoEngineMode::StandardTokioFs,
    ] {
        let engine = IoUringEngine { mode };
        assert_eq!(engine.read_chunk(&path).unwrap(), Vec::<u8>::new());
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn io_uring_temp_file_cleanup_removes_blob() {
    let engine = IoUringEngine {
        mode: IoEngineMode::StandardTokioFs,
    };
    let path = temp_path("cleanup", "a");
    engine.write_chunk(&path, b"temporary").unwrap();
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
    assert!(!path.exists());
    assert!(engine.read_chunk(&path).is_err());
}
