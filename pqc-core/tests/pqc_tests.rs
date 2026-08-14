//! Integration tests for soshal-pqc-core: ML-KEM/ML-DSA primitives, X25519
//! hybrid KEM, KEM sealing, HKDF and the Freenet identity flow.

use soshal_pqc_core::compat::make_signer_from_secret_bytes;
use soshal_pqc_core::dsa::{
    dsa_keygen, dsa_sign, dsa_sign_bytes, dsa_verify, dsa_verify_bytes, dsa_verify_hex,
};
use soshal_pqc_core::freenet_identity::{freenet_keygen, freenet_sign, freenet_verify};
use soshal_pqc_core::hkdf::hkdf_sha256;
use soshal_pqc_core::hybrid::{
    hybrid_decapsulate, hybrid_decapsulate_bytes, hybrid_encapsulate, hybrid_encapsulate_bytes,
    hybrid_keygen, hybrid_keygen_bytes, HYBRID_CT_LEN, HYBRID_PK_LEN, HYBRID_SK_LEN, SS_LEN,
};
use soshal_pqc_core::kem::{
    kem_decapsulate, kem_encapsulate, kem_keygen, kem_keygen_bytes, KEM_CT_LEN, KEM_PK_LEN,
};
use soshal_pqc_core::seal::{hybrid_seal, hybrid_unseal, kem_seal, kem_unseal};

#[test]
fn kem_keygen_lengths() {
    let (pk, sk) = kem_keygen().unwrap();
    assert_eq!(pk.len(), KEM_PK_LEN * 2);
    assert_eq!(sk.len(), 64 * 2);
    let (pk_bytes, _sk_bytes) = kem_keygen_bytes().unwrap();
    assert_eq!(pk_bytes.len(), KEM_PK_LEN);
}

#[test]
fn kem_encaps_decaps_roundtrip() {
    let (pk, sk) = kem_keygen().unwrap();
    let (ct, ss1) = kem_encapsulate(&pk).unwrap();
    assert_eq!(ct.len(), KEM_CT_LEN * 2);
    assert_eq!(ss1.len(), 32 * 2);
    let ss2 = kem_decapsulate(&ct, &sk).unwrap();
    assert_eq!(ss1, ss2);
}

#[test]
fn kem_rejects_bad_inputs() {
    assert!(kem_encapsulate("not-hex").is_err());
    assert!(kem_encapsulate("00").is_err());
    let (pk, sk) = kem_keygen().unwrap();
    let (ct, _) = kem_encapsulate(&pk).unwrap();
    assert!(kem_decapsulate(&ct, "00").is_err());
    let (pk2, _) = kem_keygen().unwrap();
    let (ct2, _) = kem_encapsulate(&pk2).unwrap();
    assert_ne!(
        kem_decapsulate(&ct2, &sk).unwrap(),
        kem_decapsulate(&ct, &sk).unwrap()
    );
}

#[test]
fn dsa_sign_verify_roundtrip() {
    let (sk, pk) = dsa_keygen(None).unwrap();
    assert!(!sk.is_empty());
    assert!(!pk.is_empty());
    let msg = b"attestation payload";
    let sig_hex = dsa_sign(msg, &sk).unwrap();
    let sig = hex::decode(&sig_hex).unwrap();
    assert!(dsa_verify(&sig, msg, &pk));
    assert!(!dsa_verify(&sig, b"tampered", &pk));
    assert!(dsa_verify_bytes(&sig, msg, &hex::decode(&pk).unwrap()));
    let sig_bytes = dsa_sign_bytes(msg, &hex::decode(&sk).unwrap()).unwrap();
    assert_eq!(sig_bytes, sig);
    assert!(dsa_verify_hex(&sig_hex, msg, &pk));
}

#[test]
fn dsa_deterministic_from_seed() {
    let seed = [0x11u8; 32];
    let (sk1, pk1) = dsa_keygen(Some(&seed)).unwrap();
    let (sk2, pk2) = dsa_keygen(Some(&seed)).unwrap();
    assert_eq!(sk1, sk2);
    assert_eq!(pk1, pk2);
    let (_, pk3) = dsa_keygen(Some(&[0x22u8; 32])).unwrap();
    assert_ne!(pk1, pk3);
}

#[test]
fn dsa_rejects_bad_pk() {
    let (sk, _) = dsa_keygen(None).unwrap();
    let sig_hex = dsa_sign(b"msg", &sk).unwrap();
    let sig = hex::decode(&sig_hex).unwrap();
    assert!(!dsa_verify(&sig, b"msg", "zzz"));
    assert!(!dsa_verify(&sig, b"msg", ""));
}

#[test]
fn hybrid_keygen_lengths_and_version() {
    let (pk, sk) = hybrid_keygen().unwrap();
    assert_eq!(pk.len(), HYBRID_PK_LEN * 2);
    assert_eq!(sk.len(), HYBRID_SK_LEN * 2);
    let (pk_bytes, sk_bytes) = hybrid_keygen_bytes().unwrap();
    assert_eq!(pk_bytes.len(), HYBRID_PK_LEN);
    assert_eq!(sk_bytes.len(), HYBRID_SK_LEN);
    assert_eq!(pk_bytes[0], 1);
}

#[test]
fn hybrid_roundtrip() {
    let (pk, sk) = hybrid_keygen().unwrap();
    let (ct, ss1) = hybrid_encapsulate(&pk, b"domain").unwrap();
    assert_eq!(ct.len(), HYBRID_CT_LEN * 2);
    assert_eq!(ss1.len(), SS_LEN * 2);
    let ss2 = hybrid_decapsulate(&ct, &sk, b"domain").unwrap();
    assert_eq!(ss1, ss2);
}

#[test]
fn hybrid_domain_mismatch_fails() {
    let (pk, sk) = hybrid_keygen().unwrap();
    let (ct, ss) = hybrid_encapsulate(&pk, b"dm:alice:bob").unwrap();
    let wrong = hybrid_decapsulate(&ct, &sk, b"dm:eve:bob").unwrap();
    assert_ne!(ss, wrong);
}

#[test]
fn hybrid_rejects_bad_lengths() {
    let (pk, sk) = hybrid_keygen().unwrap();
    assert!(hybrid_encapsulate("00", b"d").is_err());
    assert!(hybrid_encapsulate(&pk, b"d").is_ok());
    let (ct, _) = hybrid_encapsulate(&pk, b"d").unwrap();
    assert!(hybrid_decapsulate(&ct, "00", b"d").is_err());
    assert!(hybrid_decapsulate(&ct, &sk, b"d").is_ok());
}

#[test]
fn hybrid_bytes_version_rejected() {
    let mut pk = [0u8; HYBRID_PK_LEN];
    pk[0] = 0xFF;
    let mut ct = [0u8; HYBRID_CT_LEN];
    ct[0] = 0xFF;
    let sk = [0u8; HYBRID_SK_LEN];
    assert!(hybrid_encapsulate_bytes(&pk, b"d").is_err());
    assert!(hybrid_decapsulate_bytes(&ct, &sk, b"d").is_err());
}

#[test]
fn hkdf_deterministic_and_lengths() {
    let a1 = hkdf_sha256(b"ikm", b"salt", b"info", 32).unwrap();
    let a2 = hkdf_sha256(b"ikm", b"salt", b"info", 32).unwrap();
    assert_eq!(a1, a2);
    assert_eq!(a1.len(), 32);
    let b1 = hkdf_sha256(b"ikm", b"salt", b"other", 32).unwrap();
    assert_ne!(a1, b1);
    let c1 = hkdf_sha256(b"ikm", b"other", b"info", 32).unwrap();
    assert_ne!(a1, c1);
    let long = hkdf_sha256(b"ikm", b"salt", b"info", 64).unwrap();
    assert_eq!(long.len(), 64);
}

#[test]
fn kem_seal_roundtrip() {
    let (pk, sk) = kem_keygen().unwrap();
    let (ct, nonce, enc) = kem_seal(b"top secret", &pk, b"domain").unwrap();
    let pt = kem_unseal(&enc, &nonce, &ct, &sk, b"domain").unwrap();
    assert_eq!(pt, b"top secret");
    assert!(kem_unseal(&enc, &nonce, &ct, &sk, b"wrong").is_err());
}

#[test]
fn kem_seal_tamper_rejected() {
    let (pk, sk) = kem_keygen().unwrap();
    let (ct, nonce, mut enc) = kem_seal(b"payload", &pk, b"d").unwrap();
    enc.push('A');
    assert!(kem_unseal(&enc, &nonce, &ct, &sk, b"d").is_err());
    assert!(kem_unseal(&enc, "zz", &ct, &sk, b"d").is_err());
    assert!(kem_unseal(&enc, &nonce, "zz", &sk, b"d").is_err());
}

#[test]
fn kem_seal_wrong_sk_fails() {
    let (pk, sk1) = kem_keygen().unwrap();
    let (_, sk2) = kem_keygen().unwrap();
    let (ct, nonce, enc) = kem_seal(b"payload", &pk, b"d").unwrap();
    assert_ne!(sk1, sk2);
    assert!(kem_unseal(&enc, &nonce, &ct, &sk2, b"d").is_err());
}

#[test]
fn hybrid_seal_roundtrip() {
    let (pk, sk) = hybrid_keygen().unwrap();
    let (ct, nonce, enc) = hybrid_seal(b"hybrid secret", &pk, b"domain").unwrap();
    let pt = hybrid_unseal(&enc, &nonce, &ct, &sk, b"domain").unwrap();
    assert_eq!(pt, b"hybrid secret");
    assert!(hybrid_unseal(&enc, &nonce, &ct, &sk, b"wrong").is_err());
}

#[test]
fn hybrid_seal_bad_pk_len() {
    assert!(hybrid_seal(b"x", "00", b"d").is_err());
}

#[test]
fn hybrid_seal_tamper_rejected() {
    let (pk, sk) = hybrid_keygen().unwrap();
    let (ct, nonce, mut enc) = hybrid_seal(b"payload", &pk, b"d").unwrap();
    enc.push('A');
    assert!(hybrid_unseal(&enc, &nonce, &ct, &sk, b"d").is_err());
    assert!(hybrid_unseal(&enc, &nonce, "00", &sk, b"d").is_err());
}

#[test]
fn freenet_identity_keygen_shape() {
    let id = freenet_keygen(Some(&[1u8; 32])).unwrap();
    assert_eq!(id.public_key.len(), 128);
    assert_eq!(id.address, format!("free:{}", id.public_key));
    assert!(!id.private_key.is_empty());
    assert!(!id.pqc_dsa_public_key.is_empty());
    assert!(!id.pqc_kem_public_key.is_empty());
    let json = id.to_json();
    assert_eq!(json["address"], id.address);
    assert_eq!(json["pqc"]["kemPublicKey"], id.pqc_kem_public_key);
}

#[test]
fn freenet_sign_verify_roundtrip() {
    let id = freenet_keygen(None).unwrap();
    let sig = freenet_sign(b"hello freenet", &id.private_key).unwrap();
    assert!(freenet_verify(b"hello freenet", &sig, &id.public_key));
    assert!(!freenet_verify(b"tampered", &sig, &id.public_key));
}

#[test]
fn freenet_verify_rejects_garbage() {
    let id = freenet_keygen(None).unwrap();
    assert!(!freenet_verify(b"msg", "not-base64", &id.public_key));
    assert!(!freenet_verify(b"msg", "", &id.public_key));
    let sig = freenet_sign(b"msg", &id.private_key).unwrap();
    assert!(!freenet_verify(b"msg", &sig, "zzz"));
}

#[test]
fn freenet_sign_rejects_bad_key() {
    assert!(freenet_sign(b"msg", "not-base64").is_err());
}

#[test]
fn compat_signer_seed_bytes() {
    let seed = [0x42u8; 32];
    let signer = make_signer_from_secret_bytes(&seed).unwrap();
    let (sk, pk) = dsa_keygen(Some(&seed)).unwrap();
    let sig = signer(b"compat message");
    assert!(!sig.is_empty());
    assert!(dsa_verify_bytes(
        &sig,
        b"compat message",
        &hex::decode(&pk).unwrap()
    ));
    assert!(!dsa_verify_bytes(
        &sig,
        b"other",
        &hex::decode(&pk).unwrap()
    ));
    let _ = sk;
}

#[test]
fn compat_signer_rejects_short_bytes() {
    assert!(make_signer_from_secret_bytes(&[1u8, 2, 3]).is_none());
}
