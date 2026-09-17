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
use soshal_identity_core::signers::SigningOps;
use std::sync::Mutex;

/// In-process key handle. `Keys` zeroizes on drop (zeroize feature).
static SIGNER: Mutex<Option<Keys>> = Mutex::new(None);

/// Derived keys cached from the unlocked secret (LAN handshake key + at-rest
/// key). Cleared on every unlock/lock so the cache can never outlive the
/// identity it was derived from.
static LAN_KEY_CACHE: Mutex<Option<[u8; 32]>> = Mutex::new(None);
static AT_REST_KEY_CACHE: Mutex<Option<[u8; 32]>> = Mutex::new(None);

fn clear_derived_cache() {
    if let Ok(mut guard) = LAN_KEY_CACHE.lock() {
        if let Some(lan) = guard.as_mut() {
            lan.fill(0);
        }
        *guard = None;
    }
    if let Ok(mut guard) = AT_REST_KEY_CACHE.lock() {
        if let Some(rest) = guard.as_mut() {
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

use zeroize::Zeroize;

/// Unlock the signer with an nsec (or hex secret key). Accepts BOTH bech32
/// `nsec1...` and 64-char hex input via `Keys::parse`.
///
/// Returns the hex public key of the unlocked identity.
#[frb(sync, serialize)]
pub fn signer_unlock(mut secret: String) -> Result<String, String> {
    let keys = match Keys::parse(&secret) {
        Ok(k) => {
            secret.zeroize();
            k
        }
        Err(e) => {
            secret.zeroize();
            return Err(format!("invalid secret key: {e}")).into();
        }
    };
    let pk = keys.public_key().to_hex();
    // M2 fix: clear derived caches INSIDE the SIGNER lock to prevent the stale
    // identity window where another thread could call lan_key() with the old
    // key between cache clear and key replacement.
    let mut guard = crate::ffi::util::lock(&SIGNER);
    clear_derived_cache();
    soshal_crypto_core::nip44::clear_conversation_key_cache();
    soshal_identity_core::signers::clear_shared_secret_cache();
    guard.replace(keys);
    Ok(pk).into()
}

/// Lock the signer: drop the in-memory key (zeroized on drop).
///
/// Caches are cleared INSIDE the SIGNER lock, mirroring the fix applied to
/// `signer_unlock`. Without this ordering, a concurrent `lan_key()` call
/// between cache-clear and SIGNER-clear would re-derive and re-cache the LAN
/// key from the still-loaded SIGNER, leaving stale key material alive after
/// the caller's intent to lock.
#[frb(sync, serialize)]
pub fn signer_lock() -> Result<bool, String> {
    let mut guard = crate::ffi::util::lock(&SIGNER);
    clear_derived_cache();
    soshal_crypto_core::nip44::clear_conversation_key_cache();
    soshal_identity_core::signers::clear_shared_secret_cache();
    *guard = None;
    let _ = super::zap::zap_disconnect_nwc();
    let _ = super::p2p::p2p_stop_all();
    Ok(true).into()
}

/// Whether the signer currently holds an unlocked identity.
#[frb(sync, serialize)]
pub fn signer_is_locked() -> Result<bool, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
    Ok(guard.is_none()).into()
}

/// Hex public key of the unlocked identity, or an error if locked.
#[frb(sync, serialize)]
pub fn signer_pubkey() -> Result<String, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
    match guard.as_ref() {
        Some(keys) => Ok(keys.public_key().to_hex()).into(),
        None => Err("signer locked".to_string()),
    }
}

/// Caller-identity gate: the active signer must equal the pubkey a caller
/// claims to act as. Without this, any FFI call taking a `user_pubkey`
/// parameter can be made to act as an arbitrary identity by passing a
/// different pubkey.
pub(crate) fn require_identity(expected: &str) -> Result<(), String> {
    let actual = signer_pubkey()?;
    let expected_clean = expected.trim().to_ascii_lowercase();
    let actual_clean = actual.trim().to_ascii_lowercase();
    // Constant-time comparison: prevents timing side-channel on pubkey check.
    // `constant_time_eq` returns false for mismatched-length inputs.
    if !soshal_common_core::util::constant_time_eq(
        actual_clean.as_bytes(),
        expected_clean.as_bytes(),
    ) {
        return Err("identity mismatch: caller is not the claimed pubkey".to_string());
    }
    Ok(())
}

/// Constant-time match between a signer's hex pubkey and a caller-supplied
/// hex pubkey, without leaking prefix-match information via timing.
fn pubkey_matches(actual_hex: &str, expected_hex: &str) -> bool {
    let actual_clean = actual_hex.trim().to_ascii_lowercase();
    let expected_clean = expected_hex.trim().to_ascii_lowercase();
    soshal_common_core::util::constant_time_eq(actual_clean.as_bytes(), expected_clean.as_bytes())
}

/// Path to local sealed key for a given pubkey: `<db_dir>/keys/<pubkey>.key`.
fn local_sealed_key_path(pubkey: &str) -> Result<std::path::PathBuf, String> {
    super::session::validate_pubkey_hex(pubkey)?;
    let db_path = super::db::db_path()?;
    if db_path.is_empty() {
        return Err("database path not set".to_string());
    }
    let p = std::path::Path::new(&db_path);
    let parent = p
        .parent()
        .ok_or_else(|| "db_path has no parent".to_string())?;
    let keys_dir = parent.join("keys");
    let _ = std::fs::create_dir_all(&keys_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&keys_dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(keys_dir.join(format!("{pubkey}.key")))
}

/// Helper to get the 32-byte session key for sealing/unsealing local keys.
fn get_device_session_key() -> Result<[u8; 32], String> {
    let db_path = super::db::db_path()?;
    if db_path.is_empty() {
        return Err("database path not set".to_string());
    }
    let p = std::path::Path::new(&db_path);
    let parent = p
        .parent()
        .ok_or_else(|| "db_path has no parent".to_string())?;
    let session_path = parent.join("session.json");
    let key_vec = super::session::load_or_create_session_key(&session_path)?;
    key_vec
        .try_into()
        .map_err(|_| "invalid session key length".to_string())
}

/// Save sealed secret key locally.
fn save_local_sealed_key(pubkey: &str, secret: &str) -> Result<(), String> {
    let key = get_device_session_key()?;
    let path = local_sealed_key_path(pubkey)?;
    let sealed = soshal_crypto_core::at_rest::seal_at_rest(&key, secret.as_bytes())?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts
        .open(&path)
        .map_err(|e| format!("failed to open local key file: {e}"))?;
    f.write_all(sealed.as_bytes())
        .map_err(|e| format!("failed to write local key file: {e}"))?;
    f.flush()
        .map_err(|e| format!("failed to flush local key file: {e}"))?;
    Ok(())
}

/// Read sealed secret key locally.
fn read_local_sealed_key(pubkey: &str) -> Result<String, String> {
    let key = get_device_session_key()?;
    let path = local_sealed_key_path(pubkey)?;
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("local key file unreadable: {e}"))?;
    let unsealed = soshal_crypto_core::at_rest::open_at_rest(&key, content.trim())?;
    let secret = String::from_utf8(unsealed).map_err(|e| format!("invalid utf-8 secret: {e}"))?;
    Ok(secret)
}

/// Remove sealed secret key locally.
fn remove_local_sealed_key(pubkey: &str) -> bool {
    if let Ok(path) = local_sealed_key_path(pubkey) {
        if path.exists() {
            return std::fs::remove_file(path).is_ok();
        }
    }
    false
}

/// Persist the unlocked secret key to the OS keychain (desktop keyring) and local sealed storage.
/// Allows locally stored profiles to unlock without needing a recovery phrase.
#[frb(serialize)]
pub async fn signer_save_to_keyring(pubkey: String) -> Result<bool, String> {
    // Clone the secret under the guard, then release before the blocking
    // keyring write: keyring may prompt or stall, and holding the global
    // SIGNER mutex across it would stall every signing call on other threads.
    let secret = {
        let guard = crate::ffi::util::lock(&SIGNER);
        let keys = match guard.as_ref() {
            Some(k) => k,
            None => return Err("signer locked".to_string()).into(),
        };
        if !pubkey_matches(&keys.public_key().to_hex(), &pubkey) {
            return Err("pubkey does not match unlocked signer".to_string()).into();
        }
        zeroize::Zeroizing::new(keys.secret_key().to_secret_hex())
    };
    // Also save sealed secret locally so locally stored profiles never lose their keys
    let _ = save_local_sealed_key(&pubkey, &secret);
    let entry_pubkey = pubkey.clone();
    let keyring_res = tokio::task::spawn_blocking(move || {
        let entry = match keyring::Entry::new(keychain_service(), &keychain_user(&entry_pubkey)) {
            Ok(e) => e,
            Err(e) => return Err(format!("keychain unavailable: {e}")),
        };
        entry
            .set_password(&secret)
            .map_err(|e| format!("keychain write failed: {e}"))?;
        Ok::<_, String>(true)
    })
    .await
    .map_err(|e| format!("spawn_blocking join: {e}"))?;

    match keyring_res {
        Ok(_) => Ok(true).into(),
        Err(e) => {
            // OS keychain write failed; local sealed storage succeeded (saved
            // above, before the keyring attempt). Return Ok(false) so callers
            // can distinguish "OS keychain active" (true) from "local-only
            // fallback" (false) and surface an appropriate warning to the user:
            // biometric / keychain protection is NOT active in this case.
            if local_sealed_key_path(&pubkey).is_ok_and(|p| p.exists()) {
                log::warn!(
                    "signer_save_to_keyring: OS keychain unavailable ({e}); \
                     falling back to local sealed key (biometric protection inactive)"
                );
                Ok(false).into()
            } else {
                Err(e).into()
            }
        }
    }
}

/// Unlock the signer from the OS keychain or local sealed storage for the given pubkey.
#[frb(serialize)]
pub async fn signer_unlock_from_keyring(pubkey: String) -> Result<bool, String> {
    let entry_pubkey = pubkey.clone();
    let keyring_res: Result<String, String> = tokio::task::spawn_blocking(move || {
        let entry = match keyring::Entry::new(keychain_service(), &keychain_user(&entry_pubkey)) {
            Ok(e) => e,
            Err(e) => return Err(format!("keychain unavailable: {e}")),
        };
        entry
            .get_password()
            .map_err(|e| format!("no stored key: {e}"))
    })
    .await
    .map_err(|e| format!("spawn_blocking join: {e}"))?;

    let mut secret = match keyring_res {
        Ok(s) => s,
        Err(_) => {
            // Fall back to local sealed key
            read_local_sealed_key(&pubkey)?
        }
    };
    let keys = match Keys::parse(&secret) {
        Ok(k) => {
            secret.zeroize();
            k
        }
        Err(e) => {
            secret.zeroize();
            return Err(format!("stored key invalid: {e}")).into();
        }
    };
    if !pubkey_matches(&keys.public_key().to_hex(), &pubkey) {
        return Err("stored key does not match pubkey".to_string()).into();
    }
    // Clear derived caches INSIDE the SIGNER lock to prevent stale identity window.
    let mut guard = crate::ffi::util::lock(&SIGNER);
    clear_derived_cache();
    soshal_identity_core::signers::clear_shared_secret_cache();
    soshal_crypto_core::nip44::clear_conversation_key_cache();
    guard.replace(keys);
    Ok(true).into()
}

/// Bounded probe: is the OS keyring actually usable right now? A locked
/// or prompting secret-service makes writes block indefinitely. The probe
/// runs on a detached thread so a blocking keyring write cannot stall the
/// caller (a 5s recv timeout cuts it). Used by tests to skip keyring
/// assertions on headless CI or a locked desktop keyring.
pub fn keyring_available() -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Use a random probe name so the entry is unguessable by other local
        // processes and does not appear as a recognizable Soshal key in
        // keychain UIs (e.g., macOS Keychain Access, GNOME Keyring).
        let probe_name = format!("__soshal_probe_{:016x}__", rand::random::<u64>());
        let ok = match keyring::Entry::new(keychain_service(), &probe_name) {
            Ok(e) => {
                let wrote = e.set_password("probe").is_ok();
                if wrote {
                    // Retry delete once with a short delay before logging the
                    // orphan. Transient secret-service races can cause spurious
                    // first-attempt failures on GNOME Keyring / KWallet.
                    if e.delete_credential().is_err() {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        if e.delete_credential().is_err() {
                            eprintln!(
                                "[signer] keyring probe cleanup failed for {probe_name}; \
                                entry may be orphaned in the OS keychain"
                            );
                        }
                    }
                }
                wrote
            }
            Err(_) => false,
        };
        let _ = tx.send(ok);
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_default()
}

/// Remove the stored secret key for an account from the OS keychain and local sealed storage.
/// Only the currently-unlocked identity may remove its own credential.
#[frb(sync, serialize)]
pub fn signer_remove_from_keyring(pubkey: String) -> Result<bool, String> {
    // Require the caller to be the identity being removed: prevents a compromised
    // Dart layer from performing a denial-of-service by wiping a victim account's
    // stored credential without knowing the nsec.
    require_identity(&pubkey)?;
    let local_removed = remove_local_sealed_key(&pubkey);
    let entry = match keyring::Entry::new(keychain_service(), &keychain_user(&pubkey)) {
        Ok(e) => e,
        Err(e) => {
            if local_removed {
                return Ok(true).into();
            } else {
                return Err(format!("keychain unavailable: {e}")).into();
            }
        }
    };
    match entry.delete_credential() {
        Ok(_) => Ok(true).into(),
        Err(_) => Ok(local_removed).into(),
    }
}

/// Identity-derived symmetric LAN key (HKDF-SHA256 from the unlocked secret),
/// used by the P2P module for beacon MACs and chunk handshakes. Derived inside
/// the signer so key bytes never cross FFI.
///
/// # Lock ordering
/// This function acquires `LAN_KEY_CACHE` first, then `SIGNER`. All callers
/// that touch both statics must follow this order to prevent deadlocks.
pub(crate) fn lan_key() -> Result<[u8; 32], String> {
    // LOCK ORDER: LAN_KEY_CACHE → SIGNER (must not be reversed).
    let mut cache = crate::ffi::util::lock(&LAN_KEY_CACHE);
    if let Some(lan) = cache.as_ref() {
        return Ok(*lan);
    }
    let guard = crate::ffi::util::lock(&SIGNER);
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
    derived.zeroize();
    *cache = Some(out);
    Ok(out)
}

/// Identity-derived at-rest encryption key (HKDF from the unlocked secret),
/// used by domain modules to seal key material persisted in SQLite.
///
/// # Lock ordering
/// This function acquires `AT_REST_KEY_CACHE` first, then `SIGNER`.
/// Must follow the same order as `lan_key` to prevent deadlocks.
pub(crate) fn signer_at_rest_key() -> Result<[u8; 32], String> {
    // LOCK ORDER: AT_REST_KEY_CACHE → SIGNER (must not be reversed).
    let mut cache = crate::ffi::util::lock(&AT_REST_KEY_CACHE);
    if let Some(rest) = cache.as_ref() {
        return Ok(*rest);
    }
    let guard = crate::ffi::util::lock(&SIGNER);
    let keys = match guard.as_ref() {
        Some(k) => k,
        None => return Err("signer locked".to_string()),
    };
    let secret_hex = zeroize::Zeroizing::new(keys.secret_key().to_secret_hex());
    let secret = zeroize::Zeroizing::new(
        hex::decode(&*secret_hex).map_err(|e| format!("secret decode: {e}"))?,
    );
    let rest = soshal_crypto_core::at_rest::at_rest_key(&secret)?;
    *cache = Some(rest);
    Ok(rest)
}

/// Sign a Schnorr message digest (32 bytes, hex) with the unlocked key.
/// Returns the 64-byte signature as hex.
///
/// # Domain restriction (L2)
/// The underlying `sign_schnorr_digest` call uses `"blossom-auth"` as a
/// tagged-hash domain separator. This function is intentionally restricted to
/// **Blossom HTTP-auth** use (NIP-96 `Authorization: Nostr` header signing).
/// Do NOT call it for other purposes — different signing contexts require
/// different domain separators to prevent cross-protocol forgery. Add a new
/// dedicated function with the correct context tag if a new use case arises.
#[frb(sync, serialize)]
pub fn signer_schnorr_sign(message_hex: String) -> Result<String, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
    match guard.as_ref() {
        Some(keys) => {
            let msg = match hex::decode(&message_hex) {
                Ok(m) if m.len() == 32 => m,
                Ok(_) => return Err("message must be 32 bytes".to_string()).into(),
                Err(e) => return Err(format!("invalid hex: {e}")).into(),
            };
            let signer = soshal_identity_core::signers::Signer::new(keys.clone());
            let sig = signer.sign_schnorr_digest(&msg, "blossom-auth")?;
            Ok(sig).into()
        }
        None => Err("signer locked".to_string()),
    }
}

/// Sign a text message for Blossom HTTP-auth: hashes with SHA-256 then
/// Schnorr-signs the digest using the `"blossom-auth"` domain separator.
/// See `signer_schnorr_sign` for the domain restriction note.
#[frb(sync, serialize)]
pub fn signer_sign_text(message: String) -> Result<String, String> {
    let hash = soshal_crypto_core::hash::sha256_hex(message.as_bytes());
    signer_schnorr_sign(hash)
}

fn sign_event_core(keys: &Keys, unsigned: UnsignedEvent) -> Result<String, String> {
    match keys.sign_event(unsigned) {
        Ok(event) => match serde_json::to_string(&event) {
            Ok(json) => Ok(json),
            Err(e) => Err(format!("serialize: {e}")),
        },
        Err(e) => Err(format!("sign failed: {e}")),
    }
}

/// Sign a fully-formed `EventBuilder` with the unlocked key. Internal helper
/// for the domain modules (feed, messaging, relations).
pub(crate) fn sign_builder(builder: nostr::event::EventBuilder) -> Result<String, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
    match guard.as_ref() {
        Some(keys) => sign_event_core(keys, builder.finalize_unsigned(keys.public_key())),
        None => Err("signer locked".to_string()),
    }
}

/// Sign an unsigned event (NIP-59 style JSON: `pubkey`, `created_at`,
/// `kind`, `tags`, `content`; `id` optional) with the unlocked key.
/// Returns the fully signed event JSON including `id` and `sig`.
#[frb(sync, serialize)]
pub fn signer_sign_unsigned(event_json: String) -> Result<String, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
    match guard.as_ref() {
        Some(keys) => {
            let unsigned = match serde_json::from_str::<UnsignedEvent>(&event_json) {
                Ok(u) => u,
                Err(e) => return Err(format!("invalid unsigned event: {e}")).into(),
            };
            // Pre-check: verify the event's pubkey matches the active signer
            // before attempting signing. Without this, a mismatched pubkey
            // would produce an error from nostr-sdk that may expose the
            // signer's actual pubkey in its message.
            if !pubkey_matches(&keys.public_key().to_hex(), &unsigned.pubkey.to_hex()) {
                return Err("event pubkey does not match active signer".to_string()).into();
            }
            sign_event_core(keys, unsigned).into()
        }
        None => Err("signer locked".to_string()),
    }
}

/// NIP-44 v2 encrypt plaintext to `recipient_pubkey` using the unlocked key.
/// Returns the wire-format payload (base64: `2 ‖ nonce ‖ ct ‖ mac`).
#[frb(sync, serialize)]
pub fn signer_nip44_encrypt(
    mut plaintext: String,
    recipient_pubkey: String,
) -> Result<String, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
    let out = match guard.as_ref() {
        Some(keys) => {
            let pk = match PublicKey::from_hex(&recipient_pubkey) {
                Ok(p) => p,
                Err(e) => return Err(format!("invalid recipient pubkey: {e}")).into(),
            };
            nip44::encrypt(
                keys.secret_key(),
                &pk,
                plaintext.as_bytes(),
                nip44::Version::V2,
            )
        }
        None => return Err("signer locked".to_string()),
    };
    plaintext.zeroize();
    match out {
        Ok(payload) => Ok(payload).into(),
        Err(e) => Err(format!("nip44 encrypt: {e}")).into(),
    }
}

/// NIP-44 v2 decrypt a payload from `sender_pubkey` using the unlocked key.
/// Returns the plaintext String; internal NIP-44 buffers stay zeroized
/// (remove the Zeroizing<String> wrapper — frb 2.12 serializes it as an
/// opaque Dart type rather than a String, breaking the callers).
#[frb(sync, serialize)]
pub fn signer_nip44_decrypt(payload: String, sender_pubkey: String) -> Result<String, String> {
    let guard = crate::ffi::util::lock(&SIGNER);
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
    use nostr::nips::nip19::ToBech32;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_unlock_and_sign_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
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
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
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
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let alice = soshal_nostr_core::keys::generate_keys();
        let bob = soshal_nostr_core::keys::generate_keys();
        signer_unlock(alice.secret_key().to_secret_hex()).unwrap();
        let payload =
            signer_nip44_encrypt("secret dm".to_string(), bob.public_key().to_hex()).unwrap();
        signer_unlock(bob.secret_key().to_secret_hex()).unwrap();
        let plain = signer_nip44_decrypt(payload, alice.public_key().to_hex()).unwrap();
        assert_eq!(&*plain, "secret dm");
        signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_keyring_save_unlock_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        let secret = keys.secret_key().to_secret_hex();
        let pk_hex = keys.public_key().to_hex();

        if !super::keyring_available() {
            eprintln!("SKIP: OS keyring unavailable (locked or headless)");
            return;
        }
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
    #[allow(clippy::await_holding_lock)]
    async fn test_keyring_remove() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        let secret = keys.secret_key().to_secret_hex();
        let pk_hex = keys.public_key().to_hex();

        if !super::keyring_available() {
            eprintln!("SKIP: OS keyring unavailable (locked or headless)");
            return;
        }
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
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
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

    #[test]
    fn test_unlock_validation_and_schnorr_errors() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        signer_lock().unwrap();
        let err = signer_unlock("not-a-secret-key".to_string()).unwrap_err();
        assert!(
            err.starts_with("invalid secret key: "),
            "expected invalid secret key err, got: {err}"
        );
        assert!(signer_is_locked().unwrap());

        let keys = soshal_nostr_core::keys::generate_keys();
        let nsec = keys.secret_key().to_bech32().unwrap();
        assert!(nsec.starts_with("nsec1"), "nsec bech32: {nsec}");
        assert_eq!(signer_unlock(nsec).unwrap(), keys.public_key().to_hex());

        let err = signer_schnorr_sign("ab".repeat(31)).unwrap_err();
        assert_eq!(err, "message must be 32 bytes");
        let err = signer_schnorr_sign("zz".to_string()).unwrap_err();
        assert!(err.starts_with("invalid hex: "), "got: {err}");
        signer_lock().unwrap();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_keyring_validation() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk_hex = keys.public_key().to_hex();
        let other = soshal_nostr_core::keys::generate_keys();

        signer_lock().unwrap();
        let err = signer_save_to_keyring(pk_hex.clone()).await.unwrap_err();
        assert_eq!(err, "signer locked");
        signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = signer_save_to_keyring(other.public_key().to_hex())
            .await
            .unwrap_err();
        assert_eq!(err, "pubkey does not match unlocked signer");

        if let Ok(removed) = signer_remove_from_keyring(other.public_key().to_hex()) {
            assert!(!removed, "missing keychain entry must not report removal");
        }

        if !super::keyring_available() {
            eprintln!("SKIP: OS keyring unavailable (locked or headless)");
            return;
        }

        if signer_save_to_keyring(pk_hex.clone()).await.is_ok() {
            if let Ok(entry) = keyring::Entry::new(keychain_service(), &keychain_user(&pk_hex)) {
                // valid secret of a DIFFERENT key under this entry → mismatch
                if entry
                    .set_password(&other.secret_key().to_secret_hex())
                    .is_ok()
                {
                    let err = signer_unlock_from_keyring(pk_hex.clone())
                        .await
                        .unwrap_err();
                    assert!(
                        err.contains("stored key does not match pubkey"),
                        "got: {err}"
                    );
                }
                // garbage stored secret → rejected at parse
                if entry.set_password("garbage-not-a-secret").is_ok() {
                    let err = signer_unlock_from_keyring(pk_hex.clone())
                        .await
                        .unwrap_err();
                    assert!(err.starts_with("stored key invalid: "), "got: {err}");
                    let _ = signer_remove_from_keyring(pk_hex.clone());
                }
            }
        }
        signer_lock().unwrap();
    }

    #[test]
    fn test_signing_and_nip44_error_paths() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let alice = soshal_nostr_core::keys::generate_keys();
        let bob = soshal_nostr_core::keys::generate_keys();
        signer_unlock(alice.secret_key().to_secret_hex()).unwrap();

        let err = signer_sign_unsigned("not-json".to_string()).unwrap_err();
        assert!(err.starts_with("invalid unsigned event: "), "got: {err}");

        let json = serde_json::json!({
            "pubkey": bob.public_key().to_hex(),
            "created_at": soshal_common_core::format::now_secs(),
            "kind": 1,
            "tags": [],
            "content": "hi",
        })
        .to_string();
        let err = signer_sign_unsigned(json).unwrap_err();
        // Our pubkey pre-check fires before the sign attempt; the error is
        // now "event pubkey does not match active signer" rather than the
        // downstream "sign failed: …" from nostr-sdk.
        assert_eq!(
            err, "event pubkey does not match active signer",
            "got: {err}"
        );

        signer_lock().unwrap();
        let err = signer_nip44_encrypt("hi".to_string(), bob.public_key().to_hex()).unwrap_err();
        assert_eq!(err, "signer locked");
        signer_unlock(alice.secret_key().to_secret_hex()).unwrap();

        let err = signer_nip44_encrypt("hi".to_string(), "not-a-pubkey".to_string()).unwrap_err();
        assert!(err.starts_with("invalid recipient pubkey: "), "got: {err}");
        let err = signer_nip44_decrypt("AAAA".to_string(), "not-a-pubkey".to_string()).unwrap_err();
        assert!(err.starts_with("invalid sender pubkey: "), "got: {err}");

        let payload =
            signer_nip44_encrypt("secret dm".to_string(), bob.public_key().to_hex()).unwrap();
        let mut chars: Vec<char> = payload.chars().collect();
        chars[5] = if chars[5] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        let err = signer_nip44_decrypt(tampered, bob.public_key().to_hex()).unwrap_err();
        assert!(err.starts_with("nip44 decrypt: "), "got: {err}");
        signer_lock().unwrap();
    }

    #[test]
    fn test_at_rest_first_call_order() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let keys = soshal_nostr_core::keys::generate_keys();
        // at-rest FIRST (cache None branch), then lan
        signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let rest_first = signer_at_rest_key().unwrap();
        assert_ne!(rest_first, [0u8; 32]);
        let lan_after_rest = lan_key().unwrap();
        assert_eq!(
            lan_key().unwrap(),
            lan_after_rest,
            "lan cached after at-rest fill"
        );
        assert_eq!(
            signer_at_rest_key().unwrap(),
            rest_first,
            "at-rest stable after lan call"
        );
        // fresh cache (unlock clears): lan first, at-rest must derive identically
        signer_lock().unwrap();
        signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let lan_first = lan_key().unwrap();
        assert_ne!(lan_first, [0u8; 32]);
        assert_eq!(
            signer_at_rest_key().unwrap(),
            rest_first,
            "at-rest order-independent of lan-first derivation"
        );
        signer_lock().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn test_local_sealed_key_save_unlock_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let _db = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let dir = std::env::temp_dir().join(format!("soshal_signer_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("soshal.db").to_string_lossy().to_string();
        super::super::db::db_init(db_path).unwrap();

        let keys = soshal_nostr_core::keys::generate_keys();
        let pk_hex = keys.public_key().to_hex();
        signer_unlock(keys.secret_key().to_secret_hex()).unwrap();

        // Save locally sealed
        assert!(signer_save_to_keyring(pk_hex.clone()).await.unwrap());

        // Lock signer
        signer_lock().unwrap();
        assert!(signer_is_locked().unwrap());

        // Unlock without recovery phrase
        assert!(signer_unlock_from_keyring(pk_hex.clone()).await.unwrap());
        assert!(!signer_is_locked().unwrap());
        assert_eq!(signer_pubkey().unwrap(), pk_hex);

        // Remove key
        assert!(signer_remove_from_keyring(pk_hex.clone()).unwrap());
        signer_lock().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_local_sealed_key_path_traversal_rejected() {
        assert!(local_sealed_key_path("../../../etc/passwd").is_err());
        assert!(local_sealed_key_path("not_hex").is_err());
        assert!(local_sealed_key_path("").is_err());
    }

    /// M8 — Verify signer_lock clears derived caches atomically inside the
    /// SIGNER mutex. After lock+unlock, lan_key must return a fresh value
    /// matching the new signer identity, not stale material from before lock.
    #[test]
    fn test_signer_lock_clears_derived_cache_atomically() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);

        let alice = soshal_nostr_core::keys::generate_keys();
        let bob = soshal_nostr_core::keys::generate_keys();

        // Unlock as Alice and derive LAN key.
        signer_unlock(alice.secret_key().to_secret_hex()).unwrap();
        let alice_lan = lan_key().unwrap();
        assert_ne!(alice_lan, [0u8; 32]);

        // Lock then unlock as Bob.
        signer_lock().unwrap();
        signer_unlock(bob.secret_key().to_secret_hex()).unwrap();
        let bob_lan = lan_key().unwrap();
        assert_ne!(bob_lan, [0u8; 32]);

        // The LAN keys must differ — a stale cache would return Alice's key.
        assert_ne!(
            alice_lan, bob_lan,
            "signer_lock must clear LAN key cache; stale key returned"
        );

        signer_lock().unwrap();
    }

    /// L4/M5 — signer_sign_unsigned must reject events whose pubkey does not
    /// match the active signer without leaking the signer's actual pubkey in
    /// the error message.
    #[test]
    fn test_sign_unsigned_pubkey_mismatch_rejected() {
        let _g = TEST_LOCK.lock().unwrap();
        let _s = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);

        let alice = soshal_nostr_core::keys::generate_keys();
        let bob = soshal_nostr_core::keys::generate_keys();
        signer_unlock(alice.secret_key().to_secret_hex()).unwrap();

        // Unsigned event claiming Bob's pubkey, signed by Alice's key — reject.
        let json = serde_json::json!({
            "pubkey": bob.public_key().to_hex(),
            "created_at": soshal_common_core::format::now_secs(),
            "kind": 1,
            "tags": [],
            "content": "mismatch test",
        })
        .to_string();
        let err = signer_sign_unsigned(json).unwrap_err();
        assert_eq!(err, "event pubkey does not match active signer");
        // Error must NOT contain Alice's actual pubkey hex.
        assert!(
            !err.contains(&alice.public_key().to_hex()),
            "error leaks signer pubkey: {err}"
        );

        signer_lock().unwrap();
    }
}
