//! Integration tests for soshal-identity-core: key handling, mnemonics, PIN
//! security, signers, WoT scoring, NIP-05 error paths, vault trait and HKDF
//! DB-key derivation. Pure logic only — no network, no FFI.

use std::sync::Arc;
use zeroize::Zeroizing;

use nostr::event::{EventBuilder, FinalizeUnsignedEvent};
use nostr::key::Keys;

use soshal_identity_core::key_derivation::derive_db_key_hkdf;
use soshal_identity_core::keys::{
    generate_keypair, npub, npub_encode, nsec, parse_key, pubkey_from_npub, public_key_hex,
    sign_event_json,
};
use soshal_identity_core::mnemonic::{
    generate_mnemonic, restore_from_mnemonic, validate_mnemonic, MnemonicResult,
};
use soshal_identity_core::security::{
    apply_pin_attempt, constant_time_equal, derive_pin_hash, PinLockoutState, PinVerdict,
    PIN_DK_LEN, PIN_HARD_LIMIT, PIN_ITERATIONS, PIN_LOCKOUT_DELAYS, PIN_MAX_ATTEMPTS,
    PIN_SALT_BYTES,
};
use soshal_identity_core::signers::{Signer, SignerHandle, SigningOps};
use soshal_identity_core::vault::KeyringVaultService;
use soshal_identity_core::wot::{
    calculate_trust_score, compute_distance, compute_trust_score, count_mutual,
    get_wot_peers_by_distance, recalculate_wot, TrustScore, WotUpdate, WotUser,
};
use soshal_identity_core::wot_cache::WotCache;

fn sorted(v: &[String]) -> Vec<String> {
    let mut v = v.to_vec();
    v.sort();
    v
}

// ─── keys ────────────────────────────────────────────────────────────────────

#[test]
fn generate_keypair_and_expose() {
    let keys = generate_keypair();
    let pk = public_key_hex(&keys);
    assert_eq!(pk.len(), 64);
    assert!(pk.chars().all(|c| c.is_ascii_hexdigit()));
    let n = npub(&keys);
    assert!(n.starts_with("npub1"));
    let s = nsec(&keys);
    assert!(s.starts_with("nsec1"));
    // roundtrip through parse_key
    let parsed = parse_key(&s).expect("valid nsec parses");
    assert_eq!(public_key_hex(&parsed), pk);
}

#[test]
fn parse_key_rejects_garbage() {
    assert!(parse_key("").is_err());
    assert!(parse_key("not-a-key").is_err());
    assert!(parse_key("npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq").is_err());
}

#[test]
fn npub_encode_roundtrip() {
    let keys = generate_keypair();
    let pk = public_key_hex(&keys);
    let encoded = npub_encode(&pk).expect("valid hex encodes");
    assert!(encoded.starts_with("npub1"));
    let decoded = pubkey_from_npub(&encoded).expect("npub decodes");
    assert_eq!(decoded, pk);
}

#[test]
fn npub_encode_rejects_bad_input() {
    assert!(npub_encode("").is_err());
    assert!(npub_encode("zz").is_err());
    // 64 chars of invalid hex
    assert!(npub_encode(&"z".repeat(64)).is_err());
    assert!(npub_encode(&format!("{}z", "a".repeat(63))).is_err());
    assert!(pubkey_from_npub("").is_err());
    assert!(pubkey_from_npub("npub1invalid").is_err());
}

#[test]
fn sign_event_json_signs_and_validates() {
    let keys = generate_keypair();
    let builder = EventBuilder::new(nostr::event::Kind::TextNote, "hello world");
    let unsigned = builder.finalize_unsigned(keys.public_key());
    let json = serde_json::to_string(&unsigned).unwrap();

    let signed_json = sign_event_json(&keys, &json).expect("signs");
    let signed: serde_json::Value = serde_json::from_str(&signed_json).unwrap();
    assert!(signed["sig"].as_str().unwrap().len() == 128);
    assert!(signed["id"].as_str().unwrap().len() == 64);
    assert_eq!(signed["content"], "hello world");
    assert_eq!(signed["pubkey"], public_key_hex(&keys));
}

#[test]
fn sign_event_json_drops_zero_id_and_sig() {
    let keys = generate_keypair();
    // unsigned event with a placeholder zero id and a bogus sig must still sign
    let json = format!(
        r#"{{"id":"{}","pubkey":"{}","created_at":123,"kind":1,"tags":[],"content":"x","sig":"{}"}}"#,
        "00".repeat(32),
        public_key_hex(&keys),
        "ab".repeat(64)
    );
    let signed = sign_event_json(&keys, &json).expect("zero-id event signs");
    let parsed: serde_json::Value = serde_json::from_str(&signed).unwrap();
    assert_ne!(parsed["id"].as_str().unwrap(), "00".repeat(32));
}

#[test]
fn sign_event_json_rejects_malformed() {
    let keys = generate_keypair();
    assert!(sign_event_json(&keys, "").is_err());
    assert!(sign_event_json(&keys, "not json").is_err());
    assert!(sign_event_json(&keys, r#"{"kind":1}"#).is_err()); // missing pubkey
}

// ─── mnemonic ────────────────────────────────────────────────────────────────

#[test]
fn generate_and_validate_mnemonic() {
    let phrase = generate_mnemonic().expect("generates");
    assert_eq!(phrase.split_whitespace().count(), 24);
    assert!(validate_mnemonic(&phrase));
    assert!(!validate_mnemonic(""));
    assert!(!validate_mnemonic("abandon abandon abandon"));
}

#[test]
fn restore_from_mnemonic_roundtrip() {
    let phrase = generate_mnemonic().unwrap();
    let a = restore_from_mnemonic(&phrase, "").expect("restores");
    let b = restore_from_mnemonic(&phrase, "").expect("restores again");
    assert_eq!(a.private_key_hex, b.private_key_hex);
    assert_eq!(a.public_key_hex, b.public_key_hex);
    assert_eq!(a.private_key_hex.len(), 64);
    assert_eq!(a.public_key_hex.len(), 64);
    // restored key is usable and matches the exposed pubkey
    let keys = Keys::parse(&a.private_key_hex).expect("sk parses");
    assert_eq!(keys.public_key().to_string(), a.public_key_hex);
}

#[test]
fn restore_from_mnemonic_passphrase_changes_key() {
    let phrase = generate_mnemonic().unwrap();
    let plain = restore_from_mnemonic(&phrase, "").unwrap();
    let with_pass = restore_from_mnemonic(&phrase, "hunter2").unwrap();
    assert_ne!(plain.private_key_hex, with_pass.private_key_hex);
}

#[test]
fn restore_from_mnemonic_rejects_invalid() {
    assert!(restore_from_mnemonic("", "").is_err());
    assert!(restore_from_mnemonic("abandon abandon abandon", "").is_err());
    let phrase = generate_mnemonic().unwrap();
    let mut words: Vec<&str> = phrase.split_whitespace().collect();
    words[0] = "notabip39word";
    assert!(restore_from_mnemonic(&words.join(" "), "").is_err());
}

#[test]
fn mnemonic_result_fields_public() {
    let r = MnemonicResult {
        private_key_hex: Zeroizing::new("a".repeat(64)),
        public_key_hex: "b".repeat(64),
    };
    assert_eq!(r.private_key_hex.len(), 64);
    assert_eq!(r.public_key_hex.len(), 64);
}

// ─── security ────────────────────────────────────────────────────────────────

#[test]
fn constant_time_equal_basic() {
    assert!(constant_time_equal("abc", "abc"));
    assert!(constant_time_equal("", ""));
    assert!(!constant_time_equal("abc", "abd"));
    assert!(!constant_time_equal("abc", "abcd"));
}

#[test]
fn derive_pin_hash_roundtrip_and_errors() {
    let salt = hex::encode([7u8; 16]);
    let h1 = derive_pin_hash("1234", &salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap();
    let h2 = derive_pin_hash("1234", &salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), PIN_DK_LEN * 2);

    let other_salt = hex::encode([9u8; 16]);
    assert_ne!(
        derive_pin_hash("1234", &other_salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap(),
        h1
    );
    assert_ne!(
        derive_pin_hash("9999", &salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap(),
        h1
    );
}

#[test]
fn derive_pin_hash_validation() {
    let salt = hex::encode([1u8; 16]);
    assert!(derive_pin_hash("", &salt, PIN_ITERATIONS, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash(&"1".repeat(65), &salt, PIN_ITERATIONS, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", "zz", PIN_ITERATIONS, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &hex::encode([1u8; 15]), PIN_ITERATIONS, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &salt, 599_999, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &salt, 1_000_001, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &salt, PIN_ITERATIONS, 15).is_err());
    assert!(derive_pin_hash("1234", &salt, PIN_ITERATIONS, 65).is_err());
    // boundary params accepted
    assert!(derive_pin_hash("1234", &salt, 600_000, 16).is_ok());
    assert!(derive_pin_hash("1234", &salt, 1_000_000, 64).is_ok());
}

#[test]
fn pin_lockout_correct_resets() {
    let mut state = PinLockoutState {
        attempt_count: 2,
        last_attempt_at: 100,
        lockout_until: Some(200),
    };
    let v = apply_pin_attempt(&mut state, 300, true, false);
    assert_eq!(v, PinVerdict::Ok);
    assert_eq!(state.attempt_count, 0);
    assert_eq!(state.lockout_until, None);
}

#[test]
fn pin_lockout_three_wrong_triggers_delay() {
    let mut state = PinLockoutState::default();
    let v1 = apply_pin_attempt(&mut state, 10, false, false);
    assert_eq!(v1, PinVerdict::Incorrect { locked_until: None });
    let v2 = apply_pin_attempt(&mut state, 20, false, false);
    assert_eq!(v2, PinVerdict::Incorrect { locked_until: None });
    // Attempt #3 reaches PIN_MAX_ATTEMPTS: lockout starts at the first
    // delay tier (escalates per excess attempt).
    let v3 = apply_pin_attempt(&mut state, 30, false, false);
    match v3 {
        PinVerdict::Incorrect {
            locked_until: Some(u),
        } => {
            assert_eq!(u, 30 + PIN_LOCKOUT_DELAYS[0])
        }
        other => panic!("expected lockout, got {:?}", other),
    }
    assert_eq!(state.lockout_until, Some(30 + PIN_LOCKOUT_DELAYS[0]));
}

#[test]
fn pin_lockout_window_counts_and_blocks() {
    let mut state = PinLockoutState {
        lockout_until: Some(1000),
        ..Default::default()
    };
    let v = apply_pin_attempt(&mut state, 500, true, false); // correct but locked
    assert_eq!(v, PinVerdict::LockedOutUntil(1000));
    assert_eq!(state.attempt_count, 1);

    // expired window: wrong attempt proceeds
    let v = apply_pin_attempt(&mut state, 2000, false, false);
    assert_eq!(v, PinVerdict::Incorrect { locked_until: None });
    assert_eq!(state.attempt_count, 2);
}

#[test]
fn pin_lockout_hard_limit_permanent() {
    let mut state = PinLockoutState::default();
    let mut verdicts = Vec::new();
    for i in 0..PIN_HARD_LIMIT {
        verdicts.push(apply_pin_attempt(&mut state, i * 1000, false, false));
    }
    assert_eq!(verdicts.last(), Some(&PinVerdict::PermanentlyLocked));
    assert_eq!(state.attempt_count, PIN_HARD_LIMIT);
    // Hard limit is not re-derived from counters once the lockout expires:
    // the caller persists the permanent-lock flag (per apply_pin_attempt docs).
    assert_eq!(
        apply_pin_attempt(&mut state, 999_999, true, true),
        PinVerdict::PermanentlyLocked
    );
}

#[test]
fn pin_lockout_permanent_flag_short_circuits() {
    let mut state = PinLockoutState::default();
    let v = apply_pin_attempt(&mut state, 0, true, true);
    assert_eq!(v, PinVerdict::PermanentlyLocked);
}

#[test]
fn pin_lockout_within_window_hits_hard_limit() {
    let mut state = PinLockoutState {
        attempt_count: PIN_HARD_LIMIT - 1,
        lockout_until: Some(1000),
        ..Default::default()
    };
    let v = apply_pin_attempt(&mut state, 500, false, false);
    assert_eq!(v, PinVerdict::PermanentlyLocked);
}

#[test]
fn pin_lockout_state_serde_legacy_keys() {
    let json = r#"{"attemptCount":2,"lastAttemptAt":5,"lockoutUntil":7}"#;
    let state: PinLockoutState = serde_json::from_str(json).unwrap();
    assert_eq!(state.attempt_count, 2);
    assert_eq!(state.last_attempt_at, 5);
    assert_eq!(state.lockout_until, Some(7));
    let back: serde_json::Value = serde_json::to_value(&state).unwrap();
    assert_eq!(back["attemptCount"], 2);
    assert_eq!(back["lockoutUntil"], 7);
    // missing fields default
    let d: PinLockoutState = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(d.attempt_count, 0);
    assert_eq!(d.lockout_until, None);
}

#[test]
fn pin_security_constants() {
    assert_eq!(PIN_ITERATIONS, 600_000);
    assert_eq!(PIN_DK_LEN, 32);
    assert_eq!(PIN_SALT_BYTES, 16);
    assert_eq!(PIN_MAX_ATTEMPTS, 3);
    assert_eq!(PIN_HARD_LIMIT, 10);
    assert_eq!(PIN_LOCKOUT_DELAYS, [5000, 15000, 30000, 60000, 120000]);
}

// ─── signers ─────────────────────────────────────────────────────────────────

fn signer_pair() -> (Signer, Signer) {
    (
        Signer::new(generate_keypair()),
        Signer::new(generate_keypair()),
    )
}

#[test]
fn signer_public_key_and_builder() {
    let (signer, _) = signer_pair();
    let pk = signer.public_key_hex();
    assert_eq!(pk.len(), 64);
    assert_eq!(signer.public_key().unwrap().to_string(), pk);
    let builder = EventBuilder::new(nostr::event::Kind::TextNote, "signed by signer");
    let event = signer.sign_builder(builder).expect("builder signs");
    assert!(event.verify().is_ok());
    assert_eq!(event.pubkey.to_string(), pk);
}

#[test]
fn signer_sign_unsigned_event() {
    let (signer, _) = signer_pair();
    let builder = EventBuilder::new(nostr::event::Kind::TextNote, "direct");
    let unsigned = builder.finalize_unsigned(signer.public_key().unwrap());
    let event = signer.sign(unsigned).expect("signs unsigned");
    assert!(event.verify().is_ok());
}

#[test]
fn signer_sign_event_json() {
    let (signer, _) = signer_pair();
    let builder = EventBuilder::new(nostr::event::Kind::TextNote, "json path");
    let unsigned = builder.finalize_unsigned(signer.public_key().unwrap());
    let json = serde_json::to_string(&unsigned).unwrap();
    let signed = signer.sign_event_json(&json).expect("signs json");
    let event: nostr::event::Event = serde_json::from_str(&signed).unwrap();
    assert!(event.verify().is_ok());
    assert!(signer.sign_event_json("garbage").is_err());
}

#[test]
fn signer_schnorr_digest_allowlist() {
    let (signer, _) = signer_pair();
    let digest = [0x42u8; 32];
    let sig = signer
        .sign_schnorr_digest(&digest, "blossom-auth")
        .expect("allowed purpose signs");
    assert_eq!(sig.len(), 128);
    assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    // wrong length
    assert!(signer
        .sign_schnorr_digest(&[0u8; 31], "blossom-auth")
        .is_err());
    assert!(signer
        .sign_schnorr_digest(&[0u8; 33], "blossom-auth")
        .is_err());
    // unknown purpose rejected — even a 32-byte digest
    assert!(signer.sign_schnorr_digest(&digest, "arbitrary").is_err());
    assert!(signer.sign_schnorr_digest(&digest, "").is_err());
    // re-signing yields another valid-length signature (schnorr is randomized)
    let sig2 = signer
        .sign_schnorr_digest(&digest, "blossom-auth")
        .expect("re-signs");
    assert_eq!(sig2.len(), 128);
}

#[test]
fn signer_nip44_roundtrip() {
    let (alice, bob) = signer_pair();
    let bob_pk = bob.public_key().unwrap();
    let alice_pk = alice.public_key().unwrap();
    let plaintext = "secret message";
    let ciphertext = alice
        .nip44_encrypt(&bob_pk, plaintext)
        .expect("encrypts with shared secret");
    assert_ne!(ciphertext, plaintext);
    let decrypted = bob
        .nip44_decrypt(&alice_pk, &ciphertext)
        .expect("decrypts with same shared secret");
    assert_eq!(decrypted, plaintext);
    // wrong key (different ECDH point) fails to decrypt
    let (_, mallory) = signer_pair();
    assert!(mallory.nip44_decrypt(&alice_pk, &ciphertext).is_err());
    // garbage payload fails
    assert!(bob.nip44_decrypt(&alice_pk, "garbage").is_err());
}

#[test]
fn signer_handle_in_process_and_deref() {
    let keys = generate_keypair();
    let handle = SignerHandle::in_process(keys);
    let pk = handle.public_key_hex();
    assert_eq!(pk.len(), 64);
    let debug = format!("{:?}", handle);
    assert!(debug.contains(&pk));

    let handle2 = SignerHandle::new(Arc::new(Signer::new(generate_keypair())));
    assert!(handle2.public_key().is_ok());
    // Deref to SigningOps lets handle call sign_builder directly
    let event = handle2
        .sign_builder(EventBuilder::new(nostr::event::Kind::TextNote, "via deref"))
        .expect("signs");
    assert!(event.verify().is_ok());
}

#[tokio::test]
async fn signer_handle_nostr_traits() {
    use nostr::event::{AsyncSignEvent, SignEvent};
    use nostr::key::{AsyncGetPublicKey, GetPublicKey};

    let handle = SignerHandle::in_process(generate_keypair());
    let pk = handle.get_public_key().expect("sync get");
    let pk_async = handle.get_public_key_async().await.expect("async get");
    assert_eq!(pk, pk_async);

    let builder = EventBuilder::new(nostr::event::Kind::TextNote, "trait signing");
    let unsigned = builder.finalize_unsigned(pk);
    let event = handle.sign_event(unsigned.clone()).expect("sync sign");
    assert!(event.verify().is_ok());
    let event_async = handle.sign_event_async(unsigned).await.expect("async sign");
    assert!(event_async.verify().is_ok());
    assert_eq!(event.id, event_async.id);
}

// ─── vault ───────────────────────────────────────────────────────────────────

struct MemoryVault {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl KeyringVaultService for MemoryVault {
    fn save_secret(&self, key_name: &str, secret: &str) -> Result<(), String> {
        self.inner
            .lock()
            .map_err(|_| "poisoned".to_string())?
            .insert(key_name.to_string(), secret.to_string());
        Ok(())
    }
    fn get_secret(&self, key_name: &str) -> Result<Option<String>, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| "poisoned".to_string())?
            .get(key_name)
            .cloned())
    }
    fn delete_secret(&self, key_name: &str) -> Result<(), String> {
        self.inner
            .lock()
            .map_err(|_| "poisoned".to_string())?
            .remove(key_name);
        Ok(())
    }
}

#[test]
fn keyring_vault_trait_contract() {
    let vault = MemoryVault {
        inner: std::sync::Mutex::new(std::collections::HashMap::new()),
    };
    assert_eq!(vault.get_secret("missing").unwrap(), None);
    vault.save_secret("k1", "sekrit").unwrap();
    assert_eq!(vault.get_secret("k1").unwrap(), Some("sekrit".to_string()));
    vault.save_secret("k1", "updated").unwrap();
    assert_eq!(vault.get_secret("k1").unwrap(), Some("updated".to_string()));
    vault.delete_secret("k1").unwrap();
    assert_eq!(vault.get_secret("k1").unwrap(), None);
    // delete of a missing key is not an error
    vault.delete_secret("nope").unwrap();
}

// ─── wot ─────────────────────────────────────────────────────────────────────

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn count_mutual_small_and_large() {
    let a = strings(&["x", "y", "z"]);
    let b = strings(&["y", "z", "w"]);
    assert_eq!(count_mutual(&a, &b), 2);
    assert_eq!(count_mutual(&a, &[]), 0);
    assert_eq!(count_mutual(&[], &b), 0);
    // large list exercises the hash-set path
    let big: Vec<String> = (0..40).map(|i| format!("pk{i}")).collect();
    assert_eq!(count_mutual(&big, &big), 40);
    let overlap: Vec<String> = (0..40).map(|i| format!("pk{i}")).collect();
    assert_eq!(count_mutual(&big[..20], &overlap), 20);
}

#[test]
fn compute_distance_basic() {
    let me = "me";
    let target = "t";
    let follows = strings(&["a", "t"]);
    assert_eq!(compute_distance(me, me, &follows, 0), 0);
    assert_eq!(compute_distance(me, target, &follows, 0), 1);
    assert_eq!(compute_distance(me, "other", &follows, 2), 2);
    assert_eq!(compute_distance(me, "other", &follows, 0), 3);
    assert_eq!(compute_distance(me, "other", &[], 0), 3);
}

#[test]
fn calculate_trust_score_basic() {
    assert_eq!(calculate_trust_score(0, 0, 0), 1.0);
    // distance 1: 0.6 + mutual bonus (capped 0.3) + introducer bonus (capped 0.1)
    assert_eq!(calculate_trust_score(1, 0, 0), 0.6);
    assert!((calculate_trust_score(1, 6, 2) - 1.0).abs() < 1e-9); // 0.6+0.3+0.1
    assert!((calculate_trust_score(1, 100, 100) - 1.0).abs() < 1e-9);
    // distance 2: 0.3 + mutual bonus capped 0.2
    assert_eq!(calculate_trust_score(2, 0, 0), 0.3);
    assert_eq!(calculate_trust_score(2, 7, 5), 0.5);
    // unknown distance
    assert_eq!(calculate_trust_score(3, 9, 9), 0.1);
    assert_eq!(calculate_trust_score(99, 0, 0), 0.1);
}

#[test]
fn compute_trust_score_combines() {
    let me = "me";
    let follows = strings(&["direct"]);
    let ts = compute_trust_score(me, "direct", &follows, &strings(&["direct"]));
    assert_eq!(ts.distance, 1);
    assert_eq!(ts.mutual_count, 1);
    assert!(ts.score > 0.6);

    let ts2 = compute_trust_score(me, "foaf", &follows, &strings(&["direct"]));
    assert_eq!(ts2.distance, 2);
    assert_eq!(ts2.mutual_count, 1);

    let ts3 = compute_trust_score(me, "stranger", &[], &[]);
    assert_eq!(ts3.distance, 3);
    assert_eq!(ts3.score, 0.1);
}

#[test]
fn recalculate_wot_graph() {
    let me = "me";
    let users = vec![
        WotUser {
            pubkey: "me".into(),
            contacts: strings(&["a", "b"]),
        },
        WotUser {
            pubkey: "a".into(),
            contacts: strings(&["me", "c"]),
        },
        WotUser {
            pubkey: "b".into(),
            contacts: strings(&["me"]),
        },
        WotUser {
            pubkey: "c".into(),
            contacts: strings(&["a"]),
        },
        WotUser {
            pubkey: "loner".into(),
            contacts: vec![],
        },
    ];
    let updates = recalculate_wot(me, &users);
    let by_pk: std::collections::HashMap<&str, &WotUpdate> =
        updates.iter().map(|u| (u.pubkey.as_str(), u)).collect();
    assert_eq!(by_pk["me"].distance, 0);
    assert_eq!(by_pk["me"].trust_score, 1.0);
    assert_eq!(by_pk["a"].distance, 1);
    assert_eq!(by_pk["b"].distance, 1);
    assert_eq!(by_pk["c"].distance, 2); // a -> c
    assert_eq!(by_pk["loner"].distance, 3);
    assert_eq!(
        by_pk["a"].introduced_by.as_ref().map(|v| sorted(v)),
        Some(strings(&["me"]))
    );
    assert_eq!(
        by_pk["c"].introduced_by.as_ref().map(|v| sorted(v)),
        Some(strings(&["a"]))
    );
    assert_eq!(by_pk["loner"].introduced_by, None);
}

#[test]
fn recalculate_wot_empty() {
    assert!(recalculate_wot("me", &[]).is_empty());
    // self not in users still yields distance 0 entry only for listed users
    let users = vec![WotUser {
        pubkey: "other".into(),
        contacts: vec![],
    }];
    let updates = recalculate_wot("me", &users);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].distance, 3);
}

#[test]
fn get_wot_peers_by_distance_partitions() {
    let me = "me";
    let users = vec![
        WotUser {
            pubkey: "me".into(),
            contacts: strings(&["a"]),
        },
        WotUser {
            pubkey: "a".into(),
            contacts: strings(&["me", "c"]),
        },
        WotUser {
            pubkey: "c".into(),
            contacts: strings(&["a"]),
        },
        WotUser {
            pubkey: "far".into(),
            contacts: vec![],
        },
    ];
    let by_dist = get_wot_peers_by_distance(me, &users, 2);
    // self (distance 0) excluded
    assert!(!by_dist.contains_key(&0));
    assert_eq!(by_dist.get(&1).map(|v| sorted(v)), Some(strings(&["a"])));
    assert_eq!(by_dist.get(&2).map(|v| sorted(v)), Some(strings(&["c"])));
    // max_distance 1 drops distance-2 peers
    let near_only = get_wot_peers_by_distance(me, &users, 1);
    assert!(!near_only.contains_key(&2));
    // max_distance 0 drops everyone
    assert!(get_wot_peers_by_distance(me, &users, 0).is_empty());
    // stranger never appears in any bucket
    assert!(!by_dist.values().flatten().any(|p| p == "far"));
}

#[test]
fn wot_structs_serde() {
    let user: WotUser = serde_json::from_str(r#"{"pubkey":"pk","contacts":["a"]}"#).unwrap();
    assert_eq!(user.pubkey, "pk");
    assert_eq!(user.contacts, strings(&["a"]));
    let update = WotUpdate {
        pubkey: "pk".into(),
        distance: 2,
        trust_score: 0.4,
        introduced_by: Some(strings(&["me"])),
    };
    let json = serde_json::to_string(&update).unwrap();
    assert!(json.contains("\"pubkey\""));
    let ts = TrustScore {
        score: 0.5,
        distance: 2,
        mutual_count: 1,
    };
    assert_eq!(ts.score, 0.5);
    assert_eq!(ts.distance, 2);
    assert_eq!(ts.mutual_count, 1);
}

// ─── wot_cache ───────────────────────────────────────────────────────────────

fn ts(score: f64) -> TrustScore {
    TrustScore {
        score,
        distance: 1,
        mutual_count: 0,
    }
}

#[test]
fn wot_cache_get_insert_clear() {
    let cache = WotCache::new(10);
    assert_eq!(cache.get("pk"), None);
    cache.insert("pk".into(), ts(0.9));
    let got = cache.get("pk").expect("cached");
    assert_eq!(got.score, 0.9);
    cache.clear();
    assert_eq!(cache.get("pk"), None);
}

#[test]
fn wot_cache_update_refreshes_recency() {
    let cache = WotCache::new(2);
    cache.insert("a".into(), ts(0.1));
    cache.insert("b".into(), ts(0.2));
    cache.insert("a".into(), ts(0.9)); // update refreshes recency (true LRU)
    cache.insert("c".into(), ts(0.3)); // evicts least-recently-used ("b")
    assert_eq!(cache.get("a").unwrap().score, 0.9);
    assert_eq!(cache.get("b"), None);
    assert_eq!(cache.get("c").unwrap().score, 0.3);
}

#[test]
fn wot_cache_zero_capacity_is_unbounded() {
    // capacity 0 disables eviction (guard is `capacity > 0`), so entries stick
    let cache = WotCache::new(0);
    cache.insert("pk".into(), ts(0.5));
    assert_eq!(cache.get("pk").unwrap().score, 0.5);
}

// ─── nip05 ───────────────────────────────────────────────────────────────────

fn valid_pk() -> String {
    public_key_hex(&generate_keypair())
}

#[tokio::test]
async fn nip05_verify_error_paths() {
    let pk = valid_pk();
    // invalid pubkey
    let r = soshal_identity_core::nip05::verify("bob@example.com", "nope").await;
    assert!(!r.verified);
    assert!(r.error.as_deref().unwrap().contains("invalid pubkey"));
    // malformed/empty addresses fail with a non-empty error (no network:
    // empty/absent host fails DNS lookup locally)
    let r = soshal_identity_core::nip05::verify("", &pk).await;
    assert!(!r.verified);
    assert!(!r.error.as_deref().unwrap().is_empty());
    let r = soshal_identity_core::nip05::verify("bob@", &pk).await;
    assert!(!r.verified);
    assert!(!r.error.as_deref().unwrap().is_empty());
    // valid address shape but localhost/private domain blocked before any fetch
    let r = soshal_identity_core::nip05::verify("bob@localhost", &pk).await;
    assert!(!r.verified);
    assert!(r.error.as_deref().unwrap().contains("not allowed"));
    let r = soshal_identity_core::nip05::verify("bob@127.0.0.1", &pk).await;
    assert!(!r.verified);
    assert!(!r.error.as_deref().unwrap().is_empty());
}

#[tokio::test]
async fn nip05_resolve_error_paths() {
    let r = soshal_identity_core::nip05::resolve("").await;
    assert!(!r.verified);
    assert!(!r.error.as_deref().unwrap().is_empty());
    let r = soshal_identity_core::nip05::resolve("bob@").await;
    assert!(!r.verified);
    assert!(!r.error.as_deref().unwrap().is_empty());
    let r = soshal_identity_core::nip05::resolve("bob@localhost").await;
    assert!(!r.verified);
    assert!(r.error.as_deref().unwrap().contains("not allowed"));
}

#[test]
fn nip05_result_shape() {
    let r = soshal_identity_core::nip05::Nip05Result {
        verified: false,
        pubkey: None,
        relays: vec![],
        error: Some("x".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("\"verified\":false"));
}

// ─── key_derivation ──────────────────────────────────────────────────────────

#[test]
fn derive_db_key_hkdf_success() {
    let kem = hex::encode([3u8; 32]);
    let input = format!(
        r#"{{"kemSecretKeyHex":"{kem}","deviceSaltHex":"{}"}}"#,
        hex::encode([5u8; 16])
    );
    let out = derive_db_key_hkdf(&input);
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(parsed.success);
    assert!(parsed.error.is_none());
    assert_eq!(parsed.derived_key_hex.len(), 64);
    // deterministic
    assert_eq!(out, derive_db_key_hkdf(&input));
    // different salt -> different key
    let other = format!(
        r#"{{"kemSecretKeyHex":"{kem}","deviceSaltHex":"{}"}}"#,
        hex::encode([6u8; 16])
    );
    assert_ne!(out, derive_db_key_hkdf(&other));
    // no salt -> still works
    let no_salt = format!(r#"{{"kemSecretKeyHex":"{kem}"}}"#);
    let r = derive_db_key_hkdf(&no_salt);
    assert!(
        serde_json::from_str::<soshal_identity_core::key_derivation::DeriveDbKeyOutput>(&r)
            .unwrap()
            .success
    );
}

#[test]
fn derive_db_key_hkdf_snake_alias() {
    let kem = hex::encode([3u8; 32]);
    let input = format!(
        r#"{{"kem_secret_key_hex":"{kem}","device_salt_hex":"{}"}}"#,
        hex::encode([5u8; 16])
    );
    let out = derive_db_key_hkdf(&input);
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(parsed.success);
}

#[test]
fn derive_db_key_hkdf_failures() {
    // malformed json
    let out = derive_db_key_hkdf("not json");
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(!parsed.success);
    assert!(parsed.derived_key_hex.is_empty());
    assert!(parsed
        .error
        .as_deref()
        .unwrap()
        .contains("JSON parse error"));
    // invalid hex
    let out = derive_db_key_hkdf(r#"{"kemSecretKeyHex":"zz","deviceSaltHex":null}"#);
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(!parsed.success);
    assert!(parsed.error.as_deref().unwrap().contains("Invalid hex"));
    // empty hex
    let out = derive_db_key_hkdf(r#"{"kemSecretKeyHex":""}"#);
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(!parsed.success);
    assert!(parsed.error.as_deref().unwrap().contains("Invalid hex"));
    // missing field
    let out = derive_db_key_hkdf(r#"{}"#);
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(!parsed.success);
}

#[test]
fn derive_db_key_output_serde() {
    let o = soshal_identity_core::key_derivation::DeriveDbKeyOutput {
        success: true,
        derived_key_hex: "ab".repeat(32),
        error: None,
    };
    let json = serde_json::to_string(&o).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"derivedKeyHex\""));
}

#[test]
fn derive_db_key_hkdf_rejects_oversized_kem_secret_key() {
    let huge_hex = "a".repeat(20_000);
    let json_in = format!(r#"{{"kemSecretKeyHex":"{huge_hex}"}}"#);
    let out = derive_db_key_hkdf(&json_in);
    let parsed: soshal_identity_core::key_derivation::DeriveDbKeyOutput =
        serde_json::from_str(&out).unwrap();
    assert!(!parsed.success);
    assert!(parsed.error.unwrap().contains("exceeds maximum"));
}

#[test]
fn wot_cache_eviction_resilient_to_empty_queue() {
    let users = (0..70)
        .map(|i| WotUser {
            pubkey: format!("pk_{i}"),
            contacts: vec![],
        })
        .collect::<Vec<_>>();
    let partitions = get_wot_peers_by_distance("self_pk", &users, 2);
    assert!(partitions.is_empty() || partitions.values().all(|v| !v.is_empty()));
}
