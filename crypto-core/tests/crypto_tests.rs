use soshal_crypto_core::at_rest::{
    at_rest_key, at_rest_key_v2, open_at_rest, open_at_rest_v2, seal_at_rest, seal_at_rest_v2,
};
use soshal_crypto_core::base64::{
    base64_decode, base64_decode_bytes, base64_encode, base64_encode_bytes,
};
use soshal_crypto_core::base64url::{from_base64url, to_base64url};
use soshal_crypto_core::hash::{hkdf_sha256, hmac_sha256, sha256, sha256_hex};
use soshal_crypto_core::key_derivation::derive_db_key;
use soshal_crypto_core::nip44::{decrypt, encrypt, encrypt_padded, pad, unpad};
use soshal_crypto_core::pqc::{dsa, hybrid, kem};
use soshal_crypto_core::zk_trust::{generate_zk_wot_proof, verify_zk_wot_proof};

use soshal_crypto_core::pqc_ratchet::{
    decrypt_ratchet, encrypt_ratchet, init_state, ratchet_context, ratchet_header_from_tags,
    ratchet_state_from_plaintext, ratchet_state_to_json, ratchet_wrapper_tags, state_key,
    HeaderOutput, RatchetInput, RatchetOutput, RatchetState, MAX_RATCHET_WINDOW, RATCHET_VERSION,
};

#[test]
fn sha256_tests() {
    let h = sha256(b"");
    assert_eq!(
        hex::encode(h),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    let hmac_out = hmac_sha256(b"key", b"data");
    assert_eq!(hmac_out.len(), 32);
    let hkdf_out = hkdf_sha256(b"ikm", b"salt", b"info", 32).unwrap();
    assert_eq!(hkdf_out.len(), 32);
}

#[test]
fn base64_tests() {
    let s = "hello world";
    let enc = base64_encode(s);
    let dec = base64_decode(&enc);
    assert_eq!(dec, s);
    assert_eq!(base64_decode_bytes(&enc).unwrap(), s.as_bytes());
    assert_eq!(base64_encode_bytes(b"hello"), "aGVsbG8=");
    assert_eq!(base64_decode("invalid!!!"), "");

    let b64 = "aGVsbG8+/w==";
    let b64u = to_base64url(b64);
    assert_eq!(b64u, "aGVsbG8-_w");
    assert_eq!(from_base64url(&b64u), b64);
}

#[test]
fn key_derivation_test() {
    let sk = [0xABu8; 32];
    let k1 = derive_db_key(&sk, None).unwrap();
    let k2 = derive_db_key(&sk, None).unwrap();
    assert_eq!(k1, k2);
    let k_salted = derive_db_key(&sk, Some("device-1")).unwrap();
    assert_eq!(k_salted.len(), 32);
}

#[test]
fn nip44_tests() {
    let key = [0x42u8; 32];
    let plaintext = b"hello nip44";
    let ct = encrypt(plaintext, &key).unwrap();
    let pt = decrypt(&ct, &key).unwrap();
    assert_eq!(pt, plaintext);

    assert!(decrypt(&ct, &[0x00u8; 32]).is_err());
    assert!(decrypt("not-base64!!!", &key).is_err());

    assert_eq!(pad(b"short").unwrap().len(), 2 + 32);
    assert_eq!(unpad(&pad(b"hello").unwrap()).unwrap(), b"hello");

    let legacy_ct = encrypt_padded(b"legacy data", &key).unwrap();
    let encoded = base64_encode_bytes(&legacy_ct);
    assert_eq!(decrypt(&encoded, &key).unwrap(), b"legacy data");
}

#[test]
fn nip44_spec_interop_with_nostr() {
    // The spec conversation key is HKDF-extract("nip44-v2", shared_key);
    // our symmetric keys play the shared-key role. With the extracted ck
    // passed to nostr's ConversationKey::new, both implementations must
    // produce byte-identical ciphertext for the same nonce.
    use nostr::nips::nip44::v2::{decrypt_to_bytes, encrypt_to_bytes_with_nonce, ConversationKey};
    use soshal_crypto_core::base64::base64_decode_bytes;

    let key = [0x42u8; 32];
    let plaintext = b"hello spec interop";
    let nonce = [7u8; 32];

    // nostr side: ck = extract("nip44-v2", key) as conversation key.
    let ck = hmac_sha256(b"nip44-v2", &key);
    let conv = ConversationKey::new(ck);
    let nostr_ct = encrypt_to_bytes_with_nonce(&conv, plaintext, nonce).unwrap();

    // our side: force the same nonce by decrypt-reencrypt trick is not
    // possible (nonce is internal), so derive the expected payload with a
    // fixed nonce via the public API contract: our ciphertext must decode to
    // the nostr payload when decrypted by nostr.
    let our_ct = encrypt(plaintext, &key).unwrap();
    let our_bytes = base64_decode_bytes(&our_ct).unwrap();
    let nostr_pt = decrypt_to_bytes(&conv, &our_bytes).unwrap();
    assert_eq!(nostr_pt, plaintext);

    // And nostr's output must be readable by us.
    let nostr_b64 = base64_encode_bytes(&nostr_ct);
    assert_eq!(decrypt(&nostr_b64, &key).unwrap(), plaintext);
}

#[test]
fn nip44_adversarial_tests() {
    let key = [0x42u8; 32];
    let wrong_key = [0x00u8; 32];
    let plaintext = b"adversarial payload";
    let ct = encrypt(plaintext, &key).unwrap();
    let bytes = base64_decode_bytes(&ct).unwrap();

    let mut bad_mac = bytes.clone();
    let last = bad_mac.len() - 1;
    bad_mac[last] ^= 0xFF;
    bad_mac[last - 1] ^= 0xFF;
    assert!(decrypt(&base64_encode_bytes(&bad_mac), &key).is_err());

    let mut cut_end = bytes.clone();
    cut_end.truncate(cut_end.len() - 1);
    assert!(decrypt(&base64_encode_bytes(&cut_end), &key).is_err());

    let mut cut_mid = bytes.clone();
    cut_mid.drain(20..30);
    assert!(decrypt(&base64_encode_bytes(&cut_mid), &key).is_err());

    let mut bad_ct = bytes;
    let mid = bad_ct.len() / 2;
    bad_ct[mid..mid + 4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    assert!(decrypt(&base64_encode_bytes(&bad_ct), &key).is_err());

    assert!(decrypt(&ct, &wrong_key).is_err());

    let empty = encrypt_padded(b"", &key).unwrap();
    assert!(decrypt(&base64_encode_bytes(&empty), &key)
        .unwrap()
        .is_empty());

    let bin: Vec<u8> = (0..64u8).collect();
    assert_eq!(decrypt(&encrypt(&bin, &key).unwrap(), &key).unwrap(), bin);

    let padded = encrypt_padded(b"padded data", &key).unwrap();
    assert_eq!(
        decrypt(&base64_encode_bytes(&padded), &key).unwrap(),
        b"padded data"
    );

    assert!(decrypt("garbage!!$$$ not base64", &key).is_err());
}

#[test]
fn at_rest_tests() {
    let master = b"nsec1testmaster";
    let key = at_rest_key(master).unwrap();
    let sealed = seal_at_rest(&key, b"secret payload").unwrap();
    let opened = open_at_rest(&key, &sealed).unwrap();
    assert_eq!(opened, b"secret payload");

    let (pk, sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let sealed_v2 = seal_at_rest_v2(&key, &pk, b"secret payload v2").unwrap();
    let opened_v2 = open_at_rest_v2(&key, &sk, &sealed_v2).unwrap();
    assert_eq!(opened_v2, b"secret payload v2");

    let k1 = at_rest_key(b"master_secret").unwrap();
    let k2 = at_rest_key_v2(b"master_secret").unwrap();
    assert_ne!(k1, k2);
}

#[test]
fn pqc_primitives_tests() {
    let (hybrid_pk, hybrid_sk) = hybrid::keypair().unwrap();
    let (hybrid_ct, ss1) = hybrid::encapsulate(&hybrid_pk, b"domain").unwrap();
    let ss2 = hybrid::decapsulate(&hybrid_sk, &hybrid_ct, b"domain").unwrap();
    assert_eq!(ss1, ss2);

    let (kem_pk, kem_sk) = kem::keypair().unwrap();
    let (kem_ct, kss1) = kem::encapsulate(&kem_pk).unwrap();
    let kss2 = kem::decapsulate(&kem_sk, &kem_ct).unwrap();
    assert_eq!(kss1, kss2);

    let (dsa_sk, dsa_pk) = soshal_pqc_core::dsa::dsa_keygen(None).unwrap();
    assert!(!dsa_sk.is_empty());
    assert!(!dsa_pk.is_empty());
}

fn make_ratchet_pair(ctx: &str) -> (RatchetState, RatchetState) {
    let (bob_pk, bob_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let (alice_pk, alice_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let bob = init_state("", ctx, &bob_sk, &bob_pk);
    let alice = init_state(&bob_pk, ctx, &alice_sk, &alice_pk);
    (alice, bob)
}

#[test]
fn pqc_ratchet_tests() {
    let (alice, bob) = make_ratchet_pair("test-ctx");
    let (_a1, header, ciphertext) = encrypt_ratchet(&alice, "hello bob!").unwrap();
    let (_b1, plaintext) = decrypt_ratchet(&bob, &header, &ciphertext).unwrap();
    assert_eq!(plaintext, "hello bob!");
    assert_eq!(header.version, RATCHET_VERSION);

    assert_eq!(state_key("abc"), "pqc_state:abc");
    assert_eq!(ratchet_context("me", "you"), "dm:me:you");

    let tags = vec![
        vec!["ratchet_pk".into(), "pk".into()],
        vec!["ratchet_ct".into(), "ct".into()],
        vec!["ratchet_seq".into(), "5".into()],
        vec!["ratchet_cc".into(), "2".into()],
        vec!["ratchet_version".into(), "3".into()],
    ];
    let h = ratchet_header_from_tags(&tags).unwrap();
    assert_eq!(h.pk, "pk");
    assert_eq!(h.seq, 5);

    let input_state = RatchetInput {
        version: RATCHET_VERSION,
        root_key: "00".repeat(32),
        current_sk: "01".repeat(32),
        current_pk: "mypk".into(),
        peer_pk: "pk".into(),
        context: "dm:me:you".into(),
        chain_counter: 0,
        sending_chain_key: "02".repeat(32),
        sending_chain_counter: 0,
        sending_epoch_peer_pk: "pk".into(),
        sending_ct: "03".repeat(32),
        receiving_chain_key: "04".repeat(32),
        receiving_chain_counter: 0,
        skipped: vec![],
    };
    let json_st = serde_json::to_string(&input_state).unwrap();
    assert!(ratchet_state_from_plaintext(json_st.as_bytes()).is_some());

    let out_st = RatchetOutput {
        version: RATCHET_VERSION,
        root_key: "00".repeat(32),
        current_sk: "01".repeat(32),
        current_pk: "livepk".into(),
        peer_pk: "pk".into(),
        context: "dm:me:you".into(),
        chain_counter: 3,
        sending_chain_key: "02".repeat(32),
        sending_chain_counter: 7,
        sending_epoch_peer_pk: "pk".into(),
        sending_ct: "03".repeat(32),
        receiving_chain_key: "04".repeat(32),
        receiving_chain_counter: 0,
        skipped: vec![],
    };
    let out_json = ratchet_state_to_json(&out_st).unwrap();
    assert!(out_json.contains("livepk"));

    let header_out = HeaderOutput {
        version: RATCHET_VERSION,
        pk: "hdrpk".into(),
        ct: "hdrct".into(),
        seq: 7,
        chain_counter: 3,
    };
    let w_tags = ratchet_wrapper_tags(&out_st, &header_out);
    assert!(!w_tags.is_empty());
    const { assert!(MAX_RATCHET_WINDOW > 0) };
}

#[test]
fn at_rest_and_ratchet_error_paths() {
    let key = at_rest_key(b"master").unwrap();
    assert!(open_at_rest(&key, "invalid_base64_or_short").is_err());
    assert!(open_at_rest_v2(&key, "not_a_valid_sk", "short").is_err());

    // Invalid tags for ratchet header
    let invalid_tags = vec![vec!["ratchet_seq".into(), "not_an_int".into()]];
    assert!(ratchet_header_from_tags(&invalid_tags).is_none());
}

#[test]
fn kem_cross_encapsulation_tests() {
    let (kem_pk, kem_sk) = kem::keypair().unwrap();
    let (ct1, ss1) = kem::encapsulate(&kem_pk).unwrap();
    let (ct2, ss2) = kem::encapsulate(&kem_pk).unwrap();
    assert_ne!(ct1, ct2);
    assert_ne!(ss1, ss2);
    assert_eq!(kem::decapsulate(&kem_sk, &ct1).unwrap(), ss1);
    assert_eq!(kem::decapsulate(&kem_sk, &ct2).unwrap(), ss2);

    let (_other_pk, other_sk) = kem::keypair().unwrap();
    let wrong_ss = kem::decapsulate(&other_sk, &ct1).unwrap();
    assert_ne!(wrong_ss, ss1);

    let mut bad_ct = ct1;
    bad_ct[0] ^= 0x01;
    let tampered_ss = kem::decapsulate(&kem_sk, &bad_ct).unwrap();
    assert_ne!(tampered_ss, ss1);
}

#[test]
fn hybrid_domain_and_error_tests() {
    let (hybrid_pk, hybrid_sk) = hybrid::keypair().unwrap();
    let (ct, ss_a) = hybrid::encapsulate(&hybrid_pk, b"domain-a").unwrap();
    assert_eq!(
        hybrid::decapsulate(&hybrid_sk, &ct, b"domain-a").unwrap(),
        ss_a
    );
    let ss_b = hybrid::decapsulate(&hybrid_sk, &ct, b"domain-b").unwrap();
    assert_ne!(ss_b, ss_a);

    let (_other_pk, other_sk) = hybrid::keypair().unwrap();
    assert_ne!(
        hybrid::decapsulate(&other_sk, &ct, b"domain-a").unwrap(),
        ss_a
    );

    let mut bad_pk = hybrid_pk;
    bad_pk[0] = 0x00;
    assert!(hybrid::encapsulate(&bad_pk, b"domain-a").is_err());

    let mut bad_ct = ct;
    bad_ct[0] = 0x02;
    assert!(hybrid::decapsulate(&hybrid_sk, &bad_ct, b"domain-a").is_err());
}

#[allow(deprecated)]
#[test]
fn dsa_sign_verify_tests() {
    let seed = [0x13u8; 32];
    let (pk1, sk1) = dsa::keypair_from_seed(&seed).unwrap();
    let (pk2, sk2) = dsa::keypair_from_seed(&seed).unwrap();
    assert_eq!(pk1, pk2);
    assert_eq!(sk1, sk2);

    let other_seed = [0x42u8; 32];
    let (pk_other, _) = dsa::keypair_from_seed(&other_seed).unwrap();
    assert_ne!(pk_other, pk1);

    let msg = b"dsa roundtrip payload";
    let sig = dsa::sign(&seed, msg).unwrap();
    assert!(dsa::verify(&pk1, msg, &sig).is_ok());

    assert!(dsa::verify(&pk1, b"tampered message", &sig).is_err());

    let mut bad_sig = sig.clone();
    let mid = bad_sig.len() / 2;
    bad_sig[mid] ^= 0xFF;
    assert!(dsa::verify(&pk1, msg, &bad_sig).is_err());

    assert!(dsa::verify(&pk_other, msg, &sig).is_err());
}

#[test]
fn zk_trust_tests() {
    let proof = generate_zk_wot_proof("pubkey_bob", "wot_root_abc", "black_root_xy");
    assert!(verify_zk_wot_proof(&proof, "wot_root_abc", &[]));
    assert!(!verify_zk_wot_proof(&proof, "wot_root_xyz", &[]));
    assert!(!verify_zk_wot_proof(
        &proof,
        "wot_root_abc",
        std::slice::from_ref(&proof.blacklist_nullifier_hash)
    ));

    let proof2 = generate_zk_wot_proof("pubkey_bob", "wot_root_abc", "black_root_xy");
    assert_eq!(proof, proof2);

    let mut bad_b64 = proof.clone();
    bad_b64.proof_bytes_b64 = "not base64!!!".into();
    assert!(!verify_zk_wot_proof(&bad_b64, "wot_root_abc", &[]));

    let mut bad_len = proof;
    bad_len.proof_bytes_b64 = base64_encode_bytes(&[0u8; 16]);
    assert!(!verify_zk_wot_proof(&bad_len, "wot_root_abc", &[]));
}

#[test]
fn base64url_padding_tests() {
    assert_eq!(to_base64url("aGVsbG8+/w=="), "aGVsbG8-_w");
    assert_eq!(from_base64url("aGVsbG8-_w"), "aGVsbG8+/w==");

    assert_eq!(from_base64url("YWJjZA"), "YWJjZA==");
    assert_eq!(from_base64url("aGVsbG8"), "aGVsbG8=");
    assert_eq!(from_base64url("ZQ"), "ZQ==");

    assert_eq!(to_base64url(""), "");
    assert_eq!(from_base64url(""), "");

    let nopad = "aGVsbG8-_w";
    assert_eq!(to_base64url(&from_base64url(nopad)), nopad);
    let padded = "aGVsbG8+/w==";
    assert_eq!(from_base64url(&to_base64url(padded)), padded);

    assert_eq!(from_base64url("abcde"), "abcde");
}

#[test]
#[allow(deprecated)]
fn pqc_error_paths_and_seed_determinism() {
    use soshal_crypto_core::pqc::{dsa, hybrid, kem};
    // Malformed keys/ciphertexts must fail, not panic.
    let bad_pk = [0u8; hybrid::PUBLIC_KEY_LEN];
    assert!(hybrid::encapsulate(&bad_pk, b"domain").is_err());
    let (pk, sk) = hybrid::keypair().unwrap();
    let bad_ct = [0u8; hybrid::CIPHERTEXT_LEN];
    assert!(hybrid::decapsulate(&sk, &bad_ct, b"domain").is_err());
    let (ct, _ss) = hybrid::encapsulate(&pk, b"domain").unwrap();
    assert!(
        hybrid::decapsulate(&sk, &ct, b"other").is_ok(),
        "domain is unverified"
    );

    let bad_kem_pk = [0u8; kem::PUBLIC_KEY_LEN];
    // ML-KEM does not validate public-key math, so encapsulate still
    // succeeds on a zeroed key; decapsulate roundtrip below still works.
    let _ = kem::encapsulate(&bad_kem_pk);
    let (kem_pk, kem_sk) = kem::keypair().unwrap();
    let (kem_ct, kss1) = kem::encapsulate(&kem_pk).unwrap();
    let kss2 = kem::decapsulate(&kem_sk, &kem_ct).unwrap();
    assert_eq!(kss1, kss2);

    // Seeded DSA keygen is deterministic; sign/verify roundtrip.
    let seed = [42u8; dsa::SEED_LEN];
    let (pk_a, sk_a) = dsa::keypair_from_seed(&seed).unwrap();
    let (pk_b, sk_b) = dsa::keypair_from_seed(&seed).unwrap();
    assert_eq!(pk_a, pk_b);
    assert_eq!(sk_a, sk_b);
    let sig = dsa::sign(&sk_a.try_into().unwrap(), b"msg").unwrap();
    assert_eq!(sig.len(), dsa::SIGNATURE_LEN);
    dsa::verify(&pk_a, b"msg", &sig).unwrap();
    assert!(dsa::verify(&pk_a, b"other", &sig).is_err());
    // Truncated public key fails cleanly.
    assert!(dsa::verify(&pk_a[..pk_a.len() - 1], b"msg", &sig).is_err());
    assert!(dsa::verify(&pk_a, b"msg", &sig[..sig.len() - 1]).is_err());
}

#[test]
fn pqc_invalid_key_and_ciphertext_error_strings() {
    // FIPS 203 §7.2 modulus check: each 12-bit 0xFFF chunk of an all-ones
    // ek reduces mod q=3329 on decode, so the ByteEncode12 round-trip fails
    // and EncapsulationKey768::new_from_slice errors (zeroed pk round-trips,
    // so ML-KEM only validates the modulus, not key math).
    let all_ones_pk = [0xFFu8; kem::PUBLIC_KEY_LEN];
    assert_eq!(
        kem::encapsulate(&all_ones_pk).unwrap_err(),
        "invalid public key"
    );
    let zero_pk = [0u8; kem::PUBLIC_KEY_LEN];
    assert!(kem::encapsulate(&zero_pk).is_ok());

    // Wrong hybrid ct version byte reaches decapsulate's map_err.
    let (pk, sk) = hybrid::keypair().unwrap();
    let (ct, real_ss) = hybrid::encapsulate(&pk, b"domain").unwrap();
    let mut bad_ct = ct;
    bad_ct[0] = 0x02;
    assert_eq!(
        hybrid::decapsulate(&sk, &bad_ct, b"domain").unwrap_err(),
        "invalid hybrid ciphertext"
    );

    // Version-1 garbage body is NOT validated: ct parse and KDF never fail,
    // so decapsulate returns a wrong-but-valid shared secret.
    let mut garbage_ct = [0u8; hybrid::CIPHERTEXT_LEN];
    garbage_ct[0] = hybrid::VERSION;
    let wrong_ss = hybrid::decapsulate(&sk, &garbage_ct, b"domain").unwrap();
    assert_ne!(wrong_ss, real_ss);
}
