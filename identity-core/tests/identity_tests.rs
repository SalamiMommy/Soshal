//! Integration tests for soshal-identity-core: mnemonic lifecycle, key
//! derivation, NIP-05 validation paths, PIN lockout state machine, WoT
//! scoring and key helpers.

use nostr::event::FinalizeEvent;
use soshal_identity_core::key_derivation::derive_db_key_hkdf;
use soshal_identity_core::keys::{
    generate_keypair, npub, npub_encode, nsec, parse_key, pubkey_from_npub, sign_event_json,
};
use soshal_identity_core::mnemonic::{generate_mnemonic, restore_from_mnemonic, validate_mnemonic};
use soshal_identity_core::nip05::{resolve, verify};
use soshal_identity_core::security::{
    apply_pin_attempt, constant_time_equal, derive_pin_hash, PinLockoutState, PinVerdict,
    PIN_DK_LEN, PIN_HARD_LIMIT, PIN_ITERATIONS, PIN_MAX_ATTEMPTS,
};
use soshal_identity_core::wot::{
    calculate_trust_score, compute_distance, compute_trust_score, count_mutual,
    get_wot_peers_by_distance, recalculate_wot, WotUser,
};

#[test]
fn mnemonic_generate_validate_restore() {
    let phrase = generate_mnemonic().unwrap();
    assert!(validate_mnemonic(&phrase));
    assert_eq!(phrase.split_whitespace().count(), 24);
    let restored = restore_from_mnemonic(&phrase, "").unwrap();
    assert_eq!(restored.private_key_hex.len(), 64);
    assert_eq!(restored.public_key_hex.len(), 64);
    let again = restore_from_mnemonic(&phrase, "").unwrap();
    assert_eq!(restored.private_key_hex, again.private_key_hex);
    let salted = restore_from_mnemonic(&phrase, "passphrase").unwrap();
    assert_ne!(restored.private_key_hex, salted.private_key_hex);
}

#[test]
fn mnemonic_rejects_invalid() {
    assert!(!validate_mnemonic("not a valid mnemonic phrase at all"));
    assert!(!validate_mnemonic(""));
    assert!(restore_from_mnemonic("garbage words here", "").is_err());
    assert_eq!(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    );
    let known = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    assert!(validate_mnemonic(known));
    assert!(restore_from_mnemonic(known, "").is_ok());
}

#[test]
fn derive_db_key_hkdf_success_and_caps() {
    let kem_sk = "aa".repeat(64);
    let out: serde_json::Value = serde_json::from_str(&derive_db_key_hkdf(&format!(
        r#"{{"kem_secret_key_hex":"{kem_sk}"}}"#
    )))
    .unwrap();
    assert_eq!(out["success"], true);
    assert_eq!(out["derivedKeyHex"].as_str().unwrap().len(), 64);
    let out2: serde_json::Value = serde_json::from_str(&derive_db_key_hkdf(&format!(
        r#"{{"kem_secret_key_hex":"{kem_sk}","device_salt_hex":"bb"}}"#
    )))
    .unwrap();
    assert_ne!(out["derivedKeyHex"], out2["derivedKeyHex"]);
}

#[test]
fn derive_db_key_hkdf_failures() {
    let out: serde_json::Value = serde_json::from_str(&derive_db_key_hkdf("not json")).unwrap();
    assert_eq!(out["success"], false);
    assert_eq!(out["derivedKeyHex"], "");
    let out2: serde_json::Value =
        serde_json::from_str(&derive_db_key_hkdf(r#"{"kem_secret_key_hex":"zz"}"#)).unwrap();
    assert_eq!(out2["success"], false);
    let out3: serde_json::Value =
        serde_json::from_str(&derive_db_key_hkdf(r#"{"kem_secret_key_hex":123}"#)).unwrap();
    assert_eq!(out3["success"], false);
}

#[tokio::test]
async fn nip05_validation_errors_without_network() {
    let r = verify("invalid nip05 address !!", "abcd").await;
    assert!(!r.verified);
    assert!(r.error.is_some());
    let r2 = verify("user@example.com", "not-a-pubkey").await;
    assert!(!r2.verified);
    assert!(r2.error.is_some());
    let r3 = resolve("user@example.com").await;
    assert!(r3.verified || r3.error.is_some());
}

#[tokio::test]
async fn nip05_rejects_private_hostname() {
    let pk = "aa".repeat(32);
    let r = verify("user@localhost", &pk).await;
    assert!(!r.verified);
    assert!(r
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("nip05 domain is not allowed"));
}

#[tokio::test]
async fn nip05_rejects_empty_address() {
    let pk = "aa".repeat(32);
    let r = verify("", &pk).await;
    assert!(!r.verified);
    assert!(r.error.is_some());
}

#[tokio::test]
async fn nip05_resolve_rejects_private_hostname() {
    let r = resolve("user@localhost").await;
    assert!(!r.verified);
    assert!(r
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("nip05 domain is not allowed"));
}

#[tokio::test]
async fn nip05_rejects_bad_pubkey_format_before_network() {
    let r = verify("user@example.com", "not-a-pubkey").await;
    assert!(!r.verified);
    assert!(r
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("invalid pubkey"));
    let r2 = verify("user@example.com", "abcd").await;
    assert!(!r2.verified);
    assert!(r2
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("invalid pubkey"));
}

#[test]
fn keys_generate_roundtrip() {
    let keys = generate_keypair();
    let pk = soshal_identity_core::keys::public_key_hex(&keys);
    assert_eq!(pk.len(), 64);
    let n = npub(&keys);
    assert!(n.starts_with("npub1"));
    assert_eq!(pubkey_from_npub(&n).unwrap(), pk);
    assert_eq!(npub_encode(&pk).unwrap(), n);
    assert!(npub_encode("zz").is_err());
    assert!(pubkey_from_npub("npub1garbage").is_err());
    assert_eq!(nsec(&keys).len(), 63);
    assert!(parse_key("bad-key").is_err());
    let parsed = parse_key(&nsec(&keys)).unwrap();
    assert_eq!(soshal_identity_core::keys::public_key_hex(&parsed), pk);
}

#[test]
fn sign_event_json_roundtrip() {
    let keys = generate_keypair();
    let event = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, "hello world")
        .finalize(&keys)
        .unwrap();
    let mut obj = serde_json::to_value(&event).unwrap();
    obj.as_object_mut().unwrap().remove("sig");
    let signed_json = sign_event_json(&keys, &obj.to_string()).unwrap();
    let event: nostr::event::Event = serde_json::from_str(&signed_json).unwrap();
    assert!(event.verify().is_ok());
    assert_eq!(event.pubkey, keys.public_key());
    assert!(sign_event_json(&keys, "garbage").is_err());
}

#[test]
fn constant_time_compare() {
    assert!(constant_time_equal("secret", "secret"));
    assert!(!constant_time_equal("secret", "Secret"));
    assert!(!constant_time_equal("", "x"));
}

#[test]
fn pin_hash_deterministic_and_guarded() {
    let salt = hex::encode([0x42u8; 16]);
    let h1 = derive_pin_hash("1234", &salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap();
    let h2 = derive_pin_hash("1234", &salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), PIN_DK_LEN * 2);
    let h3 = derive_pin_hash("1235", &salt, PIN_ITERATIONS, PIN_DK_LEN).unwrap();
    assert_ne!(h1, h3);
    assert!(derive_pin_hash("", &salt, PIN_ITERATIONS, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &salt, 100, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &salt, 7_000_000, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &salt, PIN_ITERATIONS, 8).is_err());
    assert!(derive_pin_hash("1234", "zz", PIN_ITERATIONS, PIN_DK_LEN).is_err());
    assert!(derive_pin_hash("1234", &hex::encode([0u8; 8]), PIN_ITERATIONS, PIN_DK_LEN).is_err());
}

fn until_later(s: &mut PinLockoutState, now: i64) {
    if let Some(u) = s.lockout_until {
        if u <= now {
            s.lockout_until = None;
        }
    }
}

#[test]
fn pin_lockout_state_machine() {
    let mut s = PinLockoutState::default();
    assert_eq!(apply_pin_attempt(&mut s, 0, true, false), PinVerdict::Ok);
    assert_eq!(s.attempt_count, 0);

    let mut s = PinLockoutState::default();
    assert_eq!(
        apply_pin_attempt(&mut s, 1, false, false),
        PinVerdict::Incorrect { locked_until: None }
    );
    assert_eq!(
        apply_pin_attempt(&mut s, 2, false, false),
        PinVerdict::Incorrect { locked_until: None }
    );
    assert!(matches!(
        apply_pin_attempt(&mut s, 3, false, false),
        PinVerdict::Incorrect {
            locked_until: Some(_)
        }
    ));
    assert!(s.lockout_until.is_some());

    let until = s.lockout_until.unwrap();
    assert_eq!(
        apply_pin_attempt(&mut s, until - 1, true, false),
        PinVerdict::LockedOutUntil(until)
    );
    assert_eq!(s.attempt_count, 4);

    s.lockout_until = None;
    assert_eq!(
        apply_pin_attempt(&mut s, until + 1, true, false),
        PinVerdict::Ok
    );
    assert_eq!(s.attempt_count, 0);
    assert!(s.lockout_until.is_none());
}

#[test]
fn pin_hard_limit_permanent_lock() {
    let mut s = PinLockoutState::default();
    for i in 0..PIN_HARD_LIMIT - 1 {
        let mut st = s.clone();
        until_later(&mut st, i * 100_000);
        s = st;
        let v = apply_pin_attempt(&mut s, i * 100_000, false, false);
        assert_ne!(v, PinVerdict::PermanentlyLocked);
    }
    let mut st = s.clone();
    until_later(&mut st, 1_000_000_000);
    s = st;
    assert_eq!(
        apply_pin_attempt(&mut s, 1_000_000_000, false, false),
        PinVerdict::PermanentlyLocked
    );
    assert_eq!(
        apply_pin_attempt(&mut s, 1_000_000_001, false, false),
        PinVerdict::PermanentlyLocked
    );
    assert_eq!(
        apply_pin_attempt(&mut s, 1_000_000_002, true, true),
        PinVerdict::PermanentlyLocked
    );
}

#[test]
fn pin_max_attempts_constant() {
    assert_eq!(PIN_MAX_ATTEMPTS, 3);
}

#[test]
fn wot_mutual_count() {
    let a = vec!["x".to_string(), "y".to_string(), "z".to_string()];
    let b = vec!["y".to_string(), "z".to_string(), "w".to_string()];
    assert_eq!(count_mutual(&a, &b), 2);
    assert_eq!(count_mutual(&a, &[]), 0);
}

#[test]
fn wot_distance_calcs() {
    let a = vec!["b".to_string(), "c".to_string()];
    assert_eq!(compute_distance("me", "me", &a, 0), 0);
    assert_eq!(compute_distance("me", "b", &a, 0), 1);
    assert_eq!(compute_distance("me", "x", &a, 3), 2);
    assert_eq!(compute_distance("me", "x", &a, 0), 3);
}

#[test]
fn wot_trust_score_bounds() {
    assert_eq!(calculate_trust_score(0, 0, 0), 1.0);
    let s1 = calculate_trust_score(1, 0, 0);
    assert!((0.6..=1.0).contains(&s1));
    let s2 = calculate_trust_score(1, 100, 100);
    assert!(s2 <= 1.0);
    let s3 = calculate_trust_score(2, 0, 0);
    assert!((0.3..=0.5).contains(&s3));
    assert_eq!(calculate_trust_score(3, 0, 0), 0.1);
    let ts = compute_trust_score("me", "b", &["b".to_string()], &[]);
    assert_eq!(ts.distance, 1);
    assert_eq!(ts.mutual_count, 0);
}

#[test]
fn wot_recalculate_chain() {
    let me = "self";
    let alice = "alice";
    let bob = "bob";
    let carol = "carol";
    let users = vec![
        WotUser {
            pubkey: me.to_string(),
            contacts: vec![alice.to_string()],
        },
        WotUser {
            pubkey: alice.to_string(),
            contacts: vec![me.to_string(), bob.to_string()],
        },
        WotUser {
            pubkey: bob.to_string(),
            contacts: vec![alice.to_string(), carol.to_string()],
        },
        WotUser {
            pubkey: carol.to_string(),
            contacts: vec![],
        },
    ];
    let updates = recalculate_wot(me, &users);
    let by_pk = |pk: &str| updates.iter().find(|u| u.pubkey == pk).unwrap();
    assert_eq!(by_pk(me).distance, 0);
    assert_eq!(by_pk(alice).distance, 1);
    assert_eq!(by_pk(bob).distance, 2);
    assert_eq!(by_pk(carol).distance, 3);
    assert_eq!(by_pk(alice).introduced_by.as_ref().unwrap().len(), 1);

    let dist = get_wot_peers_by_distance(me, &users, 2);
    assert!(dist[&1].contains(&alice.to_string()));
    assert!(dist[&2].contains(&bob.to_string()));
    assert!(!dist.contains_key(&3));
}

#[test]
fn wot_handles_empty_and_self_only() {
    assert!(recalculate_wot("me", &[]).is_empty());
    let users = vec![WotUser {
        pubkey: "me".into(),
        contacts: vec![],
    }];
    let updates = recalculate_wot("me", &users);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].distance, 0);
}

#[test]
fn wot_cache_lru_and_clear() {
    use soshal_identity_core::wot::TrustScore;
    use soshal_identity_core::wot_cache::WotCache;

    let cache = WotCache::new(2);
    let s1 = TrustScore {
        distance: 1,
        mutual_count: 5,
        score: 0.8,
    };
    let s2 = TrustScore {
        distance: 2,
        mutual_count: 1,
        score: 0.4,
    };
    let s3 = TrustScore {
        distance: 3,
        mutual_count: 0,
        score: 0.1,
    };

    cache.insert("p1".into(), s1);
    cache.insert("p2".into(), s2);
    assert_eq!(cache.get("p1").unwrap().score, 0.8);

    // Evicts least-recently-used (p2); p1 was refreshed by get above
    cache.insert("p3".into(), s3);
    assert_eq!(cache.get("p1").unwrap().score, 0.8);
    assert!(cache.get("p2").is_none());
    assert_eq!(cache.get("p3").unwrap().score, 0.1);

    cache.clear();
    assert!(cache.get("p2").is_none());
}
