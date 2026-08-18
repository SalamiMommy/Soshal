//! Integration tests for soshal-pqc-core (public API coverage).
//!
//! Pure-Rust core: no FFI, no platform crates. Companion to `pqc_tests.rs`.

use ml_dsa::{MlDsa65, Seed, SigningKey};
use soshal_pqc_core::compat::{make_signer_from_secret_bytes, SignerFn};
use soshal_pqc_core::dsa::{
    dsa_keygen, dsa_sign, dsa_sign_bytes, dsa_verify, dsa_verify_bytes, dsa_verify_hex,
};
use soshal_pqc_core::freenet_identity::{freenet_keygen, freenet_sign, freenet_verify};
use soshal_pqc_core::hkdf::hkdf_sha256;
use soshal_pqc_core::hybrid::{
    hybrid_decapsulate, hybrid_decapsulate_bytes, hybrid_encapsulate, hybrid_encapsulate_bytes,
    hybrid_keygen, hybrid_keygen_bytes, DUAL_PRF_SALT, HYBRID_CT_LEN, HYBRID_PK_LEN, HYBRID_SK_LEN,
    HYBRID_VERSION, MLKEM_CT_LEN, MLKEM_PK_LEN, SS_LEN, X25519_CT_LEN, X25519_PK_LEN,
};
use soshal_pqc_core::kem::{
    kem_decapsulate, kem_decapsulate_bytes, kem_encapsulate, kem_encapsulate_bytes, kem_keygen,
    kem_keygen_bytes, KEM_CT_LEN, KEM_PK_LEN, KEM_SEED_LEN, KEM_SS_LEN,
};
use soshal_pqc_core::seal::{hybrid_seal, hybrid_unseal, kem_seal, kem_unseal};

// ---------------------------------------------------------------- kem

#[test]
fn kem_constants_and_keygen_bytes() {
    assert_eq!(KEM_PK_LEN, 1184);
    assert_eq!(KEM_CT_LEN, 1088);
    assert_eq!(KEM_SS_LEN, 32);
    assert_eq!(KEM_SEED_LEN, 64);
    let (pk, sk) = kem_keygen_bytes().unwrap();
    assert_eq!(pk.len(), KEM_PK_LEN);
    assert_eq!(sk.len(), KEM_SEED_LEN);
    let (pk2, sk2) = kem_keygen_bytes().unwrap();
    assert_ne!(pk, pk2);
    assert_ne!(sk, sk2);
}

#[test]
fn kem_bytes_roundtrip() {
    let (pk, sk) = kem_keygen_bytes().unwrap();
    let (ct, ss1) = kem_encapsulate_bytes(&pk).unwrap();
    assert_eq!(ct.len(), KEM_CT_LEN);
    assert_eq!(ss1.len(), KEM_SS_LEN);
    let ss2 = kem_decapsulate_bytes(&ct, &sk).unwrap();
    assert_eq!(ss1, ss2);
}

#[test]
fn kem_hex_roundtrip_and_uniqueness() {
    let (pk, sk) = kem_keygen().unwrap();
    assert_eq!(pk.len(), KEM_PK_LEN * 2);
    assert_eq!(sk.len(), KEM_SEED_LEN * 2);
    let (ct1, ss1) = kem_encapsulate(&pk).unwrap();
    let (ct2, ss2) = kem_encapsulate(&pk).unwrap();
    // Encapsulation is randomized: fresh ciphertext + fresh shared secret.
    assert_ne!(ct1, ct2);
    assert_ne!(ss1, ss2);
    assert_eq!(kem_decapsulate(&ct1, &sk).unwrap(), ss1);
    assert_eq!(kem_decapsulate(&ct2, &sk).unwrap(), ss2);
}

#[test]
fn kem_wrong_key_implicit_rejection() {
    let (pk, _sk) = kem_keygen().unwrap();
    let (_, sk_other) = kem_keygen().unwrap();
    let (ct, ss) = kem_encapsulate(&pk).unwrap();
    let wrong = kem_decapsulate(&ct, &sk_other).unwrap();
    assert_ne!(wrong, ss);
}

#[test]
fn kem_rejects_invalid_inputs() {
    assert!(kem_encapsulate("zz").is_err());
    assert!(kem_encapsulate("00").is_err()); // wrong length
                                             // All-zero ek decodes canonically per FIPS 203 check -> accepted.
    assert!(kem_encapsulate(&"00".repeat(KEM_PK_LEN)).is_ok());
    let (pk, _) = kem_keygen().unwrap();
    let (ct, _) = kem_encapsulate(&pk).unwrap();
    assert!(kem_decapsulate("zz", &"00".repeat(KEM_SEED_LEN)).is_err());
    assert!(kem_decapsulate(&ct, "zz").is_err());
    assert!(kem_decapsulate("00", &"00".repeat(KEM_SEED_LEN)).is_err()); // bad ct len
    assert!(kem_decapsulate(&ct, "00").is_err()); // bad sk len
}

// ---------------------------------------------------------------- dsa

#[test]
fn dsa_keygen_deterministic_and_sign_deterministic() {
    let seed = [0x5Au8; 32];
    let (sk1, pk1) = dsa_keygen(Some(&seed)).unwrap();
    let (sk2, pk2) = dsa_keygen(Some(&seed)).unwrap();
    assert_eq!(sk1, sk2);
    assert_eq!(pk1, pk2);
    // Short seed falls back to OS RNG.
    let (_, pk_rng) = dsa_keygen(Some(&[1u8, 2, 3])).unwrap();
    let (_, pk_none) = dsa_keygen(None).unwrap();
    assert_ne!(pk_rng, pk_none);
    // ML-DSA-65 is deterministic: same key + same msg -> same signature.
    let sig_a = dsa_sign(b"fixed msg", &sk1).unwrap();
    let sig_b = dsa_sign(b"fixed msg", &sk1).unwrap();
    assert_eq!(sig_a, sig_b);
    assert_ne!(sig_a, dsa_sign(b"other msg", &sk1).unwrap());
}

#[test]
fn dsa_sign_verify_bytes_roundtrip() {
    let (sk, pk) = dsa_keygen(None).unwrap();
    let msg = b"attestation";
    let sig = dsa_sign_bytes(msg, &hex::decode(&sk).unwrap()).unwrap();
    assert!(dsa_verify_bytes(&sig, msg, &hex::decode(&pk).unwrap()));
    assert!(!dsa_verify_bytes(
        &sig,
        b"tampered",
        &hex::decode(&pk).unwrap()
    ));
    assert!(!dsa_verify_bytes(&sig, msg, &[0u8; 32])); // wrong pk len
    assert!(!dsa_verify_bytes(
        &sig[..sig.len() - 1],
        msg,
        &hex::decode(&pk).unwrap()
    ));
    assert!(dsa_verify_hex(&hex::encode(&sig), msg, &pk));
}

#[test]
fn dsa_verify_rejects_garbage() {
    let (sk, pk) = dsa_keygen(None).unwrap();
    let sig = dsa_sign(b"msg", &sk).unwrap();
    assert!(!dsa_verify_hex(&sig, b"msg", "zz"));
    assert!(!dsa_verify_hex("zz", b"msg", &pk));
    assert!(!dsa_verify_hex(&sig, b"msg", &"00".repeat(64)));
    assert!(!dsa_verify(b"junk", b"msg", &pk));
    let mut sig_bytes = hex::decode(&sig).unwrap();
    let mid = sig_bytes.len() / 2;
    sig_bytes[mid] ^= 0xFF;
    assert!(!dsa_verify(&sig_bytes, b"msg", &pk));
}

#[test]
fn dsa_sign_rejects_bad_sk_hex() {
    assert!(dsa_sign(b"msg", "zz").is_none());
}

#[test]
fn dsa_keygen_short_zero_seed_falls_back_to_rng() {
    // len < 32 -> OS RNG fallback branch; must not panic.
    assert!(dsa_keygen(Some(&[0u8; 5])).is_ok());
}

// ---------------------------------------------------------------- hybrid

#[test]
fn hybrid_constants_and_keygen() {
    assert_eq!(HYBRID_VERSION, 1);
    assert_eq!(X25519_PK_LEN, 32);
    assert_eq!(X25519_CT_LEN, 32);
    assert_eq!(MLKEM_PK_LEN, KEM_PK_LEN);
    assert_eq!(MLKEM_CT_LEN, KEM_CT_LEN);
    assert_eq!(HYBRID_PK_LEN, 1 + X25519_PK_LEN + MLKEM_PK_LEN);
    assert_eq!(HYBRID_CT_LEN, 1 + X25519_CT_LEN + MLKEM_CT_LEN);
    assert_eq!(HYBRID_SK_LEN, X25519_PK_LEN + 64);
    assert_eq!(SS_LEN, 32);
    assert_eq!(DUAL_PRF_SALT, b"soshal-hybrid-v1");
    let (pk, sk) = hybrid_keygen_bytes().unwrap();
    assert_eq!(pk.len(), HYBRID_PK_LEN);
    assert_eq!(sk.len(), HYBRID_SK_LEN);
    assert_eq!(pk[0], HYBRID_VERSION);
}

#[test]
fn hybrid_bytes_roundtrip_domains() {
    let (pk, sk) = hybrid_keygen_bytes().unwrap();
    let (ct, ss1) = hybrid_encapsulate_bytes(&pk, b"dm:alice:bob").unwrap();
    assert_eq!(ct.len(), HYBRID_CT_LEN);
    assert_eq!(ct[0], HYBRID_VERSION);
    assert_eq!(ss1.len(), SS_LEN);
    let ss2 = hybrid_decapsulate_bytes(&ct, &sk, b"dm:alice:bob").unwrap();
    assert_eq!(ss1, ss2);
    // Domain is mixed into the dual-PRF: different domain -> different ss.
    let ss_wrong_domain = hybrid_decapsulate_bytes(&ct, &sk, b"dm:eve:bob").unwrap();
    assert_ne!(ss1, ss_wrong_domain);
}

#[test]
fn hybrid_hex_roundtrip() {
    let (pk, sk) = hybrid_keygen().unwrap();
    assert_eq!(pk.len(), HYBRID_PK_LEN * 2);
    assert_eq!(sk.len(), HYBRID_SK_LEN * 2);
    let (ct, ss1) = hybrid_encapsulate(&pk, b"").unwrap();
    assert_eq!(ct.len(), HYBRID_CT_LEN * 2);
    let ss2 = hybrid_decapsulate(&ct, &sk, b"").unwrap();
    assert_eq!(ss1, ss2);
}

#[test]
fn hybrid_rejects_bad_version_and_lengths() {
    let mut bad_pk = [0u8; HYBRID_PK_LEN];
    bad_pk[0] = 0x02;
    assert!(hybrid_encapsulate_bytes(&bad_pk, b"d").is_err());
    let mut bad_ct = [0u8; HYBRID_CT_LEN];
    bad_ct[0] = 0x00;
    assert!(hybrid_decapsulate_bytes(&bad_ct, &[0u8; HYBRID_SK_LEN], b"d").is_err());
    let (pk, sk) = hybrid_keygen().unwrap();
    assert!(hybrid_encapsulate("zz", b"d").is_err());
    assert!(hybrid_encapsulate("00", b"d").is_err());
    let (ct, _) = hybrid_encapsulate(&pk, b"d").unwrap();
    assert!(hybrid_decapsulate(&ct, "00", b"d").is_err());
    assert!(hybrid_decapsulate("00", &sk, b"d").is_err());
}

#[test]
fn hybrid_rejects_bad_ek_and_garbage_ct_implicit_rejection() {
    // Version byte valid but garbage ML-KEM ek: FIPS 203 canonical check fails -> Err.
    let mut bad_ek = [0xFFu8; HYBRID_PK_LEN];
    bad_ek[0] = HYBRID_VERSION;
    assert!(hybrid_encapsulate_bytes(&bad_ek, b"d").is_err());
    // Garbage ct never Errs: Ciphertext::try_from only checks length, decapsulate
    // cannot fail (implicit rejection) -> Ok with a wrong ss.
    let (pk, sk) = hybrid_keygen_bytes().unwrap();
    let (_, ss_real) = hybrid_encapsulate_bytes(&pk, b"d").unwrap();
    let mut bad_ct = [0xFFu8; HYBRID_CT_LEN];
    bad_ct[0] = HYBRID_VERSION;
    let ss_garbage = hybrid_decapsulate_bytes(&bad_ct, &sk, b"d").unwrap();
    assert_ne!(ss_garbage, ss_real);
}

// ---------------------------------------------------------------- hkdf

#[test]
fn hkdf_rfc5869_known_answer_vector() {
    // RFC 5869 A.1: SHA-256, L=42.
    let ikm: Vec<u8> = vec![0x0b; 22];
    let salt: Vec<u8> = (0..=0x0c).collect();
    let info: Vec<u8> = (0xf0..=0xf9).collect();
    let okm = hkdf_sha256(&ikm, &salt, &info, 42).unwrap();
    assert_eq!(
        hex::encode(&okm),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
}

#[test]
fn hkdf_edge_inputs() {
    let out = hkdf_sha256(b"", b"", b"", 32).unwrap();
    assert_eq!(out.len(), 32);
    let a = hkdf_sha256(b"ikm", b"", b"", 16).unwrap();
    let b = hkdf_sha256(b"ikm", b"", b"", 16).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len(), 16);
    let long = hkdf_sha256(b"ikm", b"salt", b"info", 1024).unwrap();
    assert_eq!(long.len(), 1024);
}

// ---------------------------------------------------------------- seal

#[test]
fn kem_seal_roundtrip_empty_and_large() {
    let (pk, sk) = kem_keygen().unwrap();
    let (ct, nonce, enc) = kem_seal(b"", &pk, b"domain").unwrap();
    assert_eq!(kem_unseal(&enc, &nonce, &ct, &sk, b"domain").unwrap(), b"");
    let payload = vec![0xABu8; 10_000];
    let (ct, nonce, enc) = kem_seal(&payload, &pk, b"domain").unwrap();
    assert_eq!(
        kem_unseal(&enc, &nonce, &ct, &sk, b"domain").unwrap(),
        payload
    );
    assert!(kem_unseal(&enc, &nonce, &ct, &sk, b"other-domain").is_err());
}

#[test]
fn kem_seal_rejects_bad_pk_hex() {
    assert!(kem_seal(b"x", "zz", b"d").is_err()); // bad hex
    assert!(kem_seal(b"x", "00", b"d").is_err()); // wrong length
}

#[test]
fn kem_unseal_rejects_bad_inputs() {
    let (pk, sk) = kem_keygen().unwrap();
    let (ct, nonce, enc) = kem_seal(b"payload", &pk, b"d").unwrap();
    assert!(kem_unseal("not base64!!", &nonce, &ct, &sk, b"d").is_err());
    assert!(kem_unseal(&enc, "zz", &ct, &sk, b"d").is_err());
    assert!(kem_unseal(&enc, "00", &ct, &sk, b"d").is_err()); // nonce len
    assert!(kem_unseal(&enc, &nonce, "zz", &sk, b"d").is_err());
    assert!(kem_unseal(&enc, &nonce, &ct, "zz", b"d").is_err());
    let (_, sk_other) = kem_keygen().unwrap();
    assert!(kem_unseal(&enc, &nonce, &ct, &sk_other, b"d").is_err());
}

#[test]
fn hybrid_seal_roundtrip_and_mismatch() {
    let (pk, sk) = hybrid_keygen().unwrap();
    let payload = b"hybrid sealed";
    let (ct, nonce, enc) = hybrid_seal(payload, &pk, b"dm:ctx").unwrap();
    assert_eq!(
        hybrid_unseal(&enc, &nonce, &ct, &sk, b"dm:ctx").unwrap(),
        payload
    );
    assert!(hybrid_unseal(&enc, &nonce, &ct, &sk, b"dm:other").is_err());
    let (_, sk_other) = hybrid_keygen().unwrap();
    assert!(hybrid_unseal(&enc, &nonce, &ct, &sk_other, b"dm:ctx").is_err());
}

#[test]
fn hybrid_seal_rejects_bad_inputs() {
    assert!(hybrid_seal(b"x", "zz", b"d").is_err());
    assert!(hybrid_seal(b"x", "00", b"d").is_err());
    let (pk, sk) = hybrid_keygen().unwrap();
    let (ct, nonce, enc) = hybrid_seal(b"payload", &pk, b"d").unwrap();
    assert!(hybrid_unseal("not base64!!", &nonce, &ct, &sk, b"d").is_err());
    assert!(hybrid_unseal(&enc, "00", &ct, &sk, b"d").is_err());
    assert!(hybrid_unseal(&enc, &nonce, "00", &sk, b"d").is_err());
    assert!(hybrid_unseal(&enc, &nonce, &ct, "00", b"d").is_err());
    assert!(hybrid_unseal(&enc, &nonce, "zz", &sk, b"d").is_err()); // ct bad hex
    assert!(hybrid_unseal(&enc, &nonce, &ct, "zz", b"d").is_err()); // sk bad hex
}

// ---------------------------------------------------------------- freenet identity

#[test]
fn freenet_keygen_shape_and_json() {
    let id = freenet_keygen(Some(&[0x77u8; 32])).unwrap();
    assert_eq!(id.public_key.len(), 128);
    assert_eq!(id.address, format!("free:{}", id.public_key));
    assert!(!id.private_key.is_empty());
    // ML-DSA-65: public = 1952-byte verifying key, secret = 32-byte seed-derived key.
    assert_eq!(id.pqc_dsa_public_key.len(), 1952 * 2);
    assert_eq!(id.pqc_dsa_secret_key.len(), 64);
    assert_eq!(id.pqc_kem_public_key.len(), KEM_PK_LEN * 2);
    assert_eq!(id.pqc_kem_secret_key.len(), KEM_SEED_LEN * 2);
    let json = id.to_json();
    assert_eq!(json["publicKey"], id.public_key);
    assert_eq!(json["privateKey"], id.private_key);
    assert_eq!(json["address"], id.address);
    assert_eq!(json["pqc"]["dsaPublicKey"], id.pqc_dsa_public_key);
    assert_eq!(json["pqc"]["dsaSecretKey"], id.pqc_dsa_secret_key);
    assert_eq!(json["pqc"]["kemPublicKey"], id.pqc_kem_public_key);
    assert_eq!(json["pqc"]["kemSecretKey"], id.pqc_kem_secret_key);
    // Seed determinism applies to the ML-DSA part.
    let id2 = freenet_keygen(Some(&[0x77u8; 32])).unwrap();
    assert_eq!(id.pqc_dsa_public_key, id2.pqc_dsa_public_key);
    // End-to-end with fixed keys: seed-derived secret signs, vk public verifies.
    let sig = dsa_sign(b"pqc roundtrip", &id.pqc_dsa_secret_key).unwrap();
    assert!(dsa_verify_hex(
        &sig,
        b"pqc roundtrip",
        &id.pqc_dsa_public_key
    ));
    assert!(!dsa_verify_hex(&sig, b"tampered", &id.pqc_dsa_public_key));
}

#[test]
fn freenet_sign_verify_roundtrip() {
    let id = freenet_keygen(None).unwrap();
    let msg = b"freenet message";
    let sig = freenet_sign(msg, &id.private_key).unwrap();
    assert!(freenet_verify(msg, &sig, &id.public_key));
    assert!(!freenet_verify(b"tampered", &sig, &id.public_key));
    // 65-byte uncompressed (0x04 || x || y) public key also accepted.
    let mut full = [0u8; 130];
    full[..2].copy_from_slice(b"04");
    full[2..].copy_from_slice(id.public_key.as_bytes());
    assert!(freenet_verify(
        msg,
        &sig,
        std::str::from_utf8(&full).unwrap()
    ));
}

#[test]
fn freenet_rejects_bad_keys_and_sigs() {
    assert!(freenet_sign(b"msg", "not-base64").is_err());
    assert!(freenet_sign(b"msg", "").is_err());
    let id = freenet_keygen(None).unwrap();
    let sig = freenet_sign(b"msg", &id.private_key).unwrap();
    assert!(!freenet_verify(b"msg", "not-base64", &id.public_key));
    assert!(!freenet_verify(b"msg", &sig, "zz"));
    assert!(!freenet_verify(b"msg", &sig, &"00".repeat(64)));
    assert!(!freenet_verify(b"msg", &sig, &"ab".repeat(65)));
}

// ---------------------------------------------------------------- compat

#[test]
fn compat_signer_seed_path_matches_dsa_sign() {
    let seed = [0x21u8; 32];
    let signer: SignerFn = make_signer_from_secret_bytes(&seed).unwrap();
    let (sk, pk) = dsa_keygen(Some(&seed)).unwrap();
    let msg = b"compat";
    let via_signer = signer(msg);
    let via_dsa = hex::decode(dsa_sign(msg, &sk).unwrap()).unwrap();
    assert_eq!(via_signer, via_dsa);
    assert!(dsa_verify_bytes(
        &via_signer,
        msg,
        &hex::decode(&pk).unwrap()
    ));
}

#[test]
fn compat_signer_rejects_invalid_keys() {
    assert!(make_signer_from_secret_bytes(&[]).is_none());
    assert!(make_signer_from_secret_bytes(&[1u8, 2, 3]).is_none());
    assert!(make_signer_from_secret_bytes(&[0u8; 33]).is_none());
}

#[test]
fn compat_signer_expanded_key_branch() {
    let seed = [0x42u8; 32];
    let seed_arr = Seed::try_from(seed.as_slice()).unwrap();
    let sk = SigningKey::<MlDsa65>::from_seed(&seed_arr);
    #[allow(deprecated)]
    let expanded = sk.expanded_key().to_expanded();
    assert_eq!(expanded.as_slice().len(), 4032);
    // 4032-byte input fails the 32-byte seed branch, must take the expanded branch.
    let signer: SignerFn = make_signer_from_secret_bytes(expanded.as_slice()).unwrap();
    let msg = b"expanded-key";
    let sig = signer(msg);
    assert!(sig.len() > 3000); // ML-DSA-65 signature size
    let (_, pk) = dsa_keygen(Some(&seed)).unwrap();
    assert!(dsa_verify_bytes(&sig, msg, &hex::decode(&pk).unwrap()));
    // 33-byte input matches neither branch.
    assert!(make_signer_from_secret_bytes(&[0u8; 33]).is_none());
}
