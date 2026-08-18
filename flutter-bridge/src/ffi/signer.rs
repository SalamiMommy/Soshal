//! Signer FFI module
//!
//! The only Rust-side path to key material: an nsec/secret key is accepted
//! once via `signer_unlock` (or restored from the OS keychain via
//! `signer_unlock_from_keyring`), held in-process in a Mutex, and never
//! exported to Dart. All event signing and NIP-44 encryption/decryption for
//! the active account goes through this module. Dropping the handle (or a
//! `signer_lock` call) zeroizes the in-memory key.

use flutter_rust_bridge::frb;
use nostr::event::{FinalizeUnsignedEvent, SignEvent, UnsignedEvent};
use nostr::key::{Keys, PublicKey};
use nostr::nips::nip44;
use std::sync::Mutex;

/// In-process key handle. `Keys` zeroizes on drop (zeroize feature).
static SIGNER: Mutex<Option<Keys>> = Mutex::new(None);

/// Derived keys cached from the unlocked secret (LAN handshake key + at-rest
/// key). Cleared on every unlock/lock so the cache can never outlive the
/// identity it was derived from.
static DERIVED: Mutex<Option<([u8; 32], [u8; 32])>> = Mutex::new(None);

fn clear_derived_cache() {
    if let Ok(mut guard) = DERIVED.lock() {
        if let Some((lan, rest)) = guard.as_mut() {
            lan.fill(0);
            rest.fill(0);
        }
        *guard = None;
    }
}

/// The keychain entry name for the active account's secret key, matching the
/// desktop scheme (`nsec-<pubkey>`).
fn keychain_service() -> &'static str {
    "soshal"
}

fn keychain_user(pubkey: &str) -> String {
    format!("nsec-{pubkey}")
}

/// Unlock the signer with an nsec (or hex secret key). Accepts BOTH bech32
/// `nsec1...` and 64-char hex input via `Keys::parse`.
///
/// Returns the hex public key of the unlocked identity.
#[frb(sync, serialize)]
pub fn signer_unlock(secret: String) -> Result<String, String> {
    let keys = match Keys::parse(&secret) {
        Ok(k) => k,
        Err(e) => return Err(format!("invalid secret key: {e}")).into(),
    };
    let pk = keys.public_key().to_hex();
    clear_derived_cache();
    soshal_identity_core::signers::clear_shared_secret_cache();
    SIGNER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(keys);
    Ok(pk).into()
}

/// Lock the signer: drop the in-memory key (zeroized on drop).
#[frb(sync, serialize)]
pub fn signer_lock() -> Result<bool, String> {
    clear_derived_cache();
    *SIGNER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    soshal_crypto_core::nip44::clear_conversation_key_cache();
    soshal_identity_core::signers::clear_shared_secret_cache();
    Ok(true).into()
}

/// Whether the signer currently holds an unlocked identity.
#[frb(sync, serialize)]
pub fn signer_is_locked() -> Result<bool, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    Ok(guard.is_none()).into()
}

/// Hex public key of the unlocked identity, or an error if locked.
#[frb(sync, serialize)]
pub fn signer_pubkey() -> Result<String, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(keys) => Ok(keys.public_key().to_hex()).into(),
        None => Err("signer locked".to_string()),
    }
}

/// Persist the unlocked secret key to the OS keychain (desktop keyring).
/// Only called explicitly after the user opts into "remember this device".
#[frb(serialize)]
pub async fn signer_save_to_keyring(pubkey: String) -> Result<bool, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    let keys = match guard.as_ref() {
        Some(k) => k,
        None => return Err("signer locked".to_string()).into(),
    };
    if keys.public_key().to_hex() != pubkey {
        return Err("pubkey does not match unlocked signer".to_string()).into();
    }
    let secret = zeroize::Zeroizing::new(keys.secret_key().to_secret_hex());
    let entry = match keyring::Entry::new(keychain_service(), &keychain_user(&pubkey)) {
        Ok(e) => e,
        Err(e) => return Err(format!("keychain unavailable: {e}")).into(),
    };
    match entry.set_password(&secret) {
        Ok(_) => Ok(true).into(),
        Err(e) => Err(format!("keychain write failed: {e}")).into(),
    }
}

/// Unlock the signer from the OS keychain for the given pubkey.
#[frb(serialize)]
pub async fn signer_unlock_from_keyring(pubkey: String) -> Result<bool, String> {
    let entry = match keyring::Entry::new(keychain_service(), &keychain_user(&pubkey)) {
        Ok(e) => e,
        Err(e) => return Err(format!("keychain unavailable: {e}")).into(),
    };
    let secret = match entry.get_password() {
        Ok(s) => s,
        Err(e) => return Err(format!("no stored key: {e}")).into(),
    };
    let keys = match Keys::parse(&secret) {
        Ok(k) => k,
        Err(e) => return Err(format!("stored key invalid: {e}")).into(),
    };
    if keys.public_key().to_hex() != pubkey {
        return Err("stored key does not match pubkey".to_string()).into();
    }
    clear_derived_cache();
    SIGNER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(keys);
    Ok(true).into()
}

/// Remove the stored secret key for an account from the OS keychain.
#[frb(sync, serialize)]
pub fn signer_remove_from_keyring(pubkey: String) -> Result<bool, String> {
    let entry = match keyring::Entry::new(keychain_service(), &keychain_user(&pubkey)) {
        Ok(e) => e,
        Err(e) => return Err(format!("keychain unavailable: {e}")).into(),
    };
    match entry.delete_credential() {
        Ok(_) => Ok(true).into(),
        Err(_) => Ok(false).into(),
    }
}

/// Identity-derived symmetric LAN key (HKDF-SHA256 from the unlocked secret),
/// used by the P2P module for beacon MACs and chunk handshakes. Derived inside
/// the signer so key bytes never cross FFI.
pub(crate) fn lan_key() -> Result<[u8; 32], String> {
    if let Ok(cache) = DERIVED.lock() {
        if let Some((lan, _)) = cache.as_ref() {
            return Ok(*lan);
        }
    }
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    let keys = match guard.as_ref() {
        Some(k) => k,
        None => return Err("signer locked".to_string()),
    };
    let secret_hex = zeroize::Zeroizing::new(keys.secret_key().to_secret_hex());
    let secret = zeroize::Zeroizing::new(
        hex::decode(&*secret_hex).map_err(|e| format!("secret decode: {e}"))?,
    );
    let mut derived = soshal_crypto_core::hash::hkdf_sha256(
        &secret,
        b"soshal-lan-salt-v1",
        b"soshal-lan-key-v1",
        32,
    )
    .map_err(|e| format!("lan key derive: {e}"))?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&derived);
    // Wipe the intermediate buffers (no zeroize dep in the bridge crate).
    for b in derived.iter_mut() {
        *b = 0;
    }
    if let Ok(mut cache) = DERIVED.lock() {
        if cache.is_none() {
            *cache = Some((out, [0u8; 32]));
        }
    }
    Ok(out)
}

/// Identity-derived at-rest encryption key (HKDF from the unlocked secret),
/// used by domain modules to seal key material persisted in SQLite.
pub(crate) fn signer_at_rest_key() -> Result<[u8; 32], String> {
    if let Ok(cache) = DERIVED.lock() {
        if let Some((_, rest)) = cache.as_ref() {
            if *rest != [0u8; 32] {
                return Ok(*rest);
            }
        }
    }
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    let keys = match guard.as_ref() {
        Some(k) => k,
        None => return Err("signer locked".to_string()),
    };
    let secret_hex = zeroize::Zeroizing::new(keys.secret_key().to_secret_hex());
    let secret = zeroize::Zeroizing::new(
        hex::decode(&*secret_hex).map_err(|e| format!("secret decode: {e}"))?,
    );
    let rest = soshal_crypto_core::at_rest::at_rest_key(&secret)?;
    if let Ok(mut cache) = DERIVED.lock() {
        match cache.as_mut() {
            Some((_lan, slot)) => *slot = rest,
            None => *cache = Some(([0u8; 32], rest)),
        }
    }
    Ok(rest)
}

/// Sign a Schnorr message digest (32 bytes, hex) with the unlocked key.
/// Returns the 64-byte signature as hex.
#[frb(sync, serialize)]
pub fn signer_schnorr_sign(message_hex: String) -> Result<String, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(keys) => {
            let msg = match hex::decode(&message_hex) {
                Ok(m) if m.len() == 32 => m,
                Ok(_) => return Err("message must be 32 bytes".to_string()).into(),
                Err(e) => return Err(format!("invalid hex: {e}")).into(),
            };
            let sig = keys.sign_schnorr(msg);
            Ok(sig.to_hex()).into()
        }
        None => Err("signer locked".to_string()),
    }
}

/// Sign a text message: hashes with SHA-256 then Schnorr-signs the digest in Rust.
#[frb(sync, serialize)]
pub fn signer_sign_text(message: String) -> Result<String, String> {
    let hash = soshal_crypto_core::hash::sha256_hex(message.as_bytes());
    signer_schnorr_sign(hash)
}

/// Sign a fully-formed `EventBuilder` with the unlocked key. Internal helper
/// for the domain modules (feed, messaging, relations).
pub(crate) fn sign_builder(builder: nostr::event::EventBuilder) -> Result<String, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(keys) => match keys.sign_event(builder.finalize_unsigned(keys.public_key())) {
            Ok(event) => match serde_json::to_string(&event) {
                Ok(json) => Ok(json),
                Err(e) => Err(format!("serialize: {e}")),
            },
            Err(e) => Err(format!("sign failed: {e}")),
        },
        None => Err("signer locked".to_string()),
    }
}

/// Sign an unsigned event (NIP-59 style JSON: `pubkey`, `created_at`,
/// `kind`, `tags`, `content`; `id` optional) with the unlocked key.
/// Returns the fully signed event JSON including `id` and `sig`.
#[frb(sync, serialize)]
pub fn signer_sign_unsigned(event_json: String) -> Result<String, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(keys) => {
            let unsigned = match serde_json::from_str::<UnsignedEvent>(&event_json) {
                Ok(u) => u,
                Err(e) => return Err(format!("invalid unsigned event: {e}")).into(),
            };
            match keys.sign_event(unsigned) {
                Ok(event) => match serde_json::to_string(&event) {
                    Ok(json) => Ok(json).into(),
                    Err(e) => Err(format!("serialize: {e}")).into(),
                },
                Err(e) => Err(format!("sign failed: {e}")).into(),
            }
        }
        None => Err("signer locked".to_string()),
    }
}

/// NIP-44 v2 encrypt plaintext to `recipient_pubkey` using the unlocked key.
/// Returns the wire-format payload (base64: `2 ‖ nonce ‖ ct ‖ mac`).
#[frb(sync, serialize)]
pub fn signer_nip44_encrypt(plaintext: String, recipient_pubkey: String) -> Result<String, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(keys) => {
            let pk = match PublicKey::from_hex(&recipient_pubkey) {
                Ok(p) => p,
                Err(e) => return Err(format!("invalid recipient pubkey: {e}")).into(),
            };
            match nip44::encrypt(
                keys.secret_key(),
                &pk,
                plaintext.as_bytes(),
                nip44::Version::V2,
            ) {
                Ok(payload) => Ok(payload).into(),
                Err(e) => Err(format!("nip44 encrypt: {e}")).into(),
            }
        }
        None => Err("signer locked".to_string()),
    }
}

/// NIP-44 v2 decrypt a payload from `sender_pubkey` using the unlocked key.
#[frb(sync, serialize)]
pub fn signer_nip44_decrypt(payload: String, sender_pubkey: String) -> Result<String, String> {
    let guard = SIGNER.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(keys) => {
            let pk = match PublicKey::from_hex(&sender_pubkey) {
                Ok(p) => p,
                Err(e) => return Err(format!("invalid sender pubkey: {e}")).into(),
            };
            match nip44::decrypt(keys.secret_key(), &pk, &payload) {
                Ok(plaintext) => Ok(plaintext).into(),
                Err(e) => Err(format!("nip44 decrypt: {e}")).into(),
            }
        }
        None => Err("signer locked".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_unlock_and_sign_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        let secret = keys.secret_key().to_secret_hex();
        let pk_hex = keys.public_key().to_hex();

        let unlocked = signer_unlock(secret).unwrap();
        assert_eq!(unlocked, pk_hex);
        assert!(!signer_is_locked().unwrap());

        let msg = [7u8; 32];
        let sig = signer_schnorr_sign(hex::encode(msg)).unwrap();
        assert_eq!(sig.len(), 128);
        signer_lock().unwrap();
        assert!(signer_is_locked().unwrap());
        assert!(signer_pubkey().is_err());
    }

    #[test]
    fn test_unsigned_event_sign() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        let json = "{\"pubkey\":\"\",\"created_at\":0,\"kind\":1,\"tags\":[],\"content\":\"hi\"}"
            .to_string();
        // build with correct pubkey
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["pubkey"] = serde_json::json!(keys.public_key().to_hex());
        v["created_at"] = serde_json::json!(soshal_common_core::format::now_secs());
        signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let signed = signer_sign_unsigned(v.to_string()).unwrap();
        let ev: serde_json::Value = serde_json::from_str(&signed).unwrap();
        assert!(ev.get("sig").is_some());
        assert!(ev.get("id").is_some());
        signer_lock().unwrap();
    }

    #[test]
    fn test_nip44_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let alice = soshal_nostr_core::keys::generate_keys();
        let bob = soshal_nostr_core::keys::generate_keys();
        signer_unlock(alice.secret_key().to_secret_hex()).unwrap();
        let payload =
            signer_nip44_encrypt("secret dm".to_string(), bob.public_key().to_hex()).unwrap();
        signer_unlock(bob.secret_key().to_secret_hex()).unwrap();
        let plain = signer_nip44_decrypt(payload, alice.public_key().to_hex()).unwrap();
        assert_eq!(plain, "secret dm");
        signer_lock().unwrap();
    }

    #[tokio::test]
    async fn test_keyring_save_unlock_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        let secret = keys.secret_key().to_secret_hex();
        let pk_hex = keys.public_key().to_hex();

        signer_unlock(secret).unwrap();
        assert_eq!(signer_pubkey().unwrap(), pk_hex);
        match signer_save_to_keyring(pk_hex.clone()).await {
            Ok(_) => {
                signer_lock().unwrap();
                assert!(signer_unlock_from_keyring(pk_hex.clone()).await.unwrap());
                assert!(!signer_is_locked().unwrap());
                let sig = signer_sign_text("keyring roundtrip".to_string()).unwrap();
                assert_eq!(sig.len(), 128);
            }
            Err(e) => assert!(!e.is_empty()),
        }
        signer_lock().unwrap();
        let _ = signer_remove_from_keyring(pk_hex);
    }

    #[tokio::test]
    async fn test_keyring_remove() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        let secret = keys.secret_key().to_secret_hex();
        let pk_hex = keys.public_key().to_hex();

        signer_unlock(secret).unwrap();
        let _ = signer_save_to_keyring(pk_hex.clone()).await;
        match signer_remove_from_keyring(pk_hex.clone()) {
            Ok(_) => {}
            Err(e) => assert!(!e.is_empty()),
        }
        assert!(signer_unlock_from_keyring(pk_hex).await.is_err());
        signer_lock().unwrap();
    }

    #[test]
    fn test_derived_keys_locked_and_unlocked() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert!(lan_key().is_err());
        assert!(signer_at_rest_key().is_err());
        let keys = soshal_nostr_core::keys::generate_keys();
        signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let lan1 = lan_key().unwrap();
        assert_eq!(lan1.len(), 32);
        assert_eq!(lan_key().unwrap(), lan1, "lan key cached");
        let rest1 = signer_at_rest_key().unwrap();
        assert_ne!(rest1, [0u8; 32], "at-rest key must be non-zero");
        assert_eq!(signer_at_rest_key().unwrap(), rest1, "at-rest key cached");
        clear_derived_cache();
        assert_eq!(lan_key().unwrap(), lan1, "lan key stable across cache wipe");
        assert_eq!(signer_at_rest_key().unwrap(), rest1, "at-rest key stable");
        signer_lock().unwrap();
    }
}
