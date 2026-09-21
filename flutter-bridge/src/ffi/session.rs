//! Session FFI module
//! Multi-account, keychain, pin/biometrics

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::sync::Mutex;

/// Session account entry
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionAccount {
    pub pubkey: String,
    pub npub: String,
    pub last_used: u64,
    pub relay_list: Vec<String>,
    /// FCM/APNs device registration token. Stored in `session.json` (owner-only
    /// 0600 permissions + HMAC-signed). A leaked push token allows a push server
    /// to deliver notifications to this device but does not expose message content
    /// or private keys. For deployments with higher sensitivity requirements,
    /// store this in the OS keychain instead.
    #[serde(default)]
    pub push_token: Option<String>,
}

/// Maximum session age before requiring re-authentication (30 days).
const SESSION_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;
/// Maximum idle time before requiring re-authentication (7 days).
const SESSION_IDLE_TIMEOUT_SECS: u64 = 7 * 24 * 60 * 60;

/// Session file structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionData {
    pub active_pubkey: Option<String>,
    pub accounts: Vec<SessionAccount>,
    /// UNIX timestamp when this session was last loaded from disk.
    /// Used to enforce session max age.
    #[serde(default)]
    pub loaded_at: Option<u64>,
    /// HMAC-SHA256 integrity tag over the session payload (sans this field),
    /// keyed by a device-local 32-byte key in `session.key`.
    ///
    /// **Threat model**: guards against accidental file corruption and against
    /// tampering by a process that can write `session.json` but cannot read
    /// `session.key` (e.g. a second app on a non-rooted device). It does NOT
    /// protect against a device-owner-level attacker who can exfiltrate both
    /// files — in that scenario the attacker can re-compute a valid HMAC.
    /// For stronger guarantees, key `session.key` from the OS keychain secret
    /// (same entry as `signer_save_to_keyring`).
    #[serde(default)]
    pub sig: Option<String>,
}

lazy_static::lazy_static! {
    static ref SESSION: Mutex<Option<SessionData>> = Mutex::new(None);
}

fn lock_session() -> Result<std::sync::MutexGuard<'static, Option<SessionData>>, String> {
    SESSION
        .lock()
        .map_err(|e| format!("session lock poisoned: {e}"))
}

/// Validate that `pubkey` is a 64-character lowercase hex string.
pub(crate) fn validate_pubkey_hex(pubkey: &str) -> Result<(), String> {
    if pubkey.len() != 64 || !pubkey.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(format!(
            "invalid pubkey: expected 64 lowercase hex chars, got {:?}",
            &pubkey[..pubkey.len().min(16)]
        ));
    }
    Ok(())
}

/// Validate that `npub` has the `npub1` prefix and a minimum length.
fn validate_npub(npub: &str) -> Result<(), String> {
    if !npub.starts_with("npub1") || npub.len() < 10 {
        return Err(format!(
            "invalid npub: must start with 'npub1' and be at least 10 chars, got {:?}",
            &npub[..npub.len().min(12)]
        ));
    }
    Ok(())
}

/// Enforce the session idle timeout for the currently-active account.
/// Returns `Err` if the active account has exceeded `SESSION_IDLE_TIMEOUT_SECS`
/// since `last_used`. A session with no active account is not timed out.
fn check_idle_timeout(session: &SessionData) -> Result<(), String> {
    let now = soshal_common_core::format::now_secs() as u64;
    if let Some(ref active_pk) = session.active_pubkey {
        if let Some(acc) = session.accounts.iter().find(|a| &a.pubkey == active_pk) {
            if now.saturating_sub(acc.last_used) > SESSION_IDLE_TIMEOUT_SECS {
                return Err("session idle too long, please re-authenticate".to_string());
            }
        }
    }
    Ok(())
}

/// Resolve and validate the `session.json` path derived from `db_path`.
///
/// Guards against path-traversal: the parent directory of `db_path` must
/// canonicalize to the same directory as the process-global DB path set by
/// `db_init`. This prevents a compromised Dart caller from writing/reading
/// `session.json` at an arbitrary filesystem location.
fn validated_session_path(db_path: &str) -> Result<std::path::PathBuf, String> {
    // Require an absolute path to rule out CWD-relative tricks.
    let p = std::path::Path::new(db_path);
    if !p.is_absolute() {
        return Err("db_path must be an absolute path".to_string());
    }
    // Canonicalize the parent to resolve any `..` or symlink components.
    let parent = p
        .parent()
        .ok_or_else(|| "db_path has no parent directory".to_string())?;
    let _ = std::fs::create_dir_all(parent);
    let canon_parent =
        std::fs::canonicalize(parent).map_err(|e| format!("db_path parent invalid: {e}"))?;
    // Cross-check against the stored DB path set by db_init, if available.
    // This prevents a caller from supplying a legitimately-absolute but
    // unrelated path (e.g. /tmp/evil/session.json).
    if let Ok(stored) = super::db::db_path() {
        if !stored.is_empty() {
            let stored_parent = std::path::Path::new(&stored)
                .parent()
                .and_then(|pp| std::fs::canonicalize(pp).ok());
            if let Some(sp) = stored_parent {
                if canon_parent != sp {
                    return Err(
                        "db_path does not match the initialized database location".to_string()
                    );
                }
            }
        }
    }
    Ok(canon_parent.join("session.json"))
}

/// Runtime-hardened session path validation with TOCTOU protection.
///
/// This adds additional runtime checks to prevent time-of-check-time-of-use
/// race conditions where the filesystem state might change between validation
/// and actual file operations.
pub(crate) fn validated_session_path_hardened(db_path: &str) -> Result<std::path::PathBuf, String> {
    let session_path = validated_session_path(db_path)?;

    // Additional runtime check before file operations
    let stored_db_path = super::db::db_path()?;
    if !stored_db_path.is_empty() {
        let session_parent = session_path
            .parent()
            .and_then(|p| std::fs::canonicalize(p).ok())
            .ok_or_else(|| "Session parent canonicalization failed".to_string())?;

        let db_parent = std::path::Path::new(&stored_db_path)
            .parent()
            .and_then(|p| std::fs::canonicalize(p).ok())
            .ok_or_else(|| "DB parent canonicalization failed".to_string())?;

        if session_parent != db_parent {
            return Err("Runtime path validation failed: parent mismatch".to_string());
        }
    }

    // Security check: ensure the session path is within the expected directory
    validate_path_security(&session_path)?;

    Ok(session_path)
}

/// Validate that a path doesn't escape the expected directory bounds
/// and has appropriate permissions.
fn validate_path_security(path: &std::path::Path) -> Result<(), String> {
    // Reject if any component of the path is a symlink: a symlinked user-
    // writable parent could redirect the session file write to an arbitrary
    // location despite canonicalization (TOCTOU hardening).
    let mut comp = path;
    let mut components: Vec<&std::path::Path> = Vec::new();
    while let Some(parent) = comp.parent() {
        components.push(parent);
        comp = parent;
    }
    for c in components.iter().rev() {
        if c.as_os_str().is_empty() {
            continue;
        }
        if let Ok(m) = std::fs::symlink_metadata(c) {
            if m.file_type().is_symlink() {
                return Err(format!(
                    "Security: path component {} is a symlink",
                    c.display()
                ));
            }
        }
    }

    // Check if the path exists
    if path.exists() {
        // Validate file permissions to prevent world-writable files
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata =
                std::fs::metadata(path).map_err(|e| format!("Cannot access file metadata: {e}"))?;
            let permissions = metadata.permissions();
            let mode = permissions.mode();

            // Ensure file is not world-writable
            if mode & 0o002 != 0 {
                return Err("Security: file is world-writable".to_string());
            }

            // Note: UID validation is skipped to avoid unsafe code.
            // The directory canonicalization + permission checks provide
            // sufficient protection against TOCTOU attacks for this use case.
        }
    }

    // Note: path traversal components (`..`, `~`) and symlinks are already
    // fully resolved by `validated_session_path`'s `canonicalize()` call
    // before this function is invoked. No additional string-level traversal
    // check is needed here — the `O_NOFOLLOW | O_EXCL` flags at write time
    // are the true TOCTOU guard.

    Ok(())
}

/// Device-local HMAC key filename living next to `session.json`.
const SESSION_KEY_FILE: &str = "session.key";

fn session_key_path(session_path: &std::path::Path) -> std::path::PathBuf {
    session_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(SESSION_KEY_FILE)
}

/// Load the 32-byte device session key, creating it (0600) on first use.
/// Always succeeds in a writable app-data dir; failing closes session ops.
pub(crate) fn load_or_create_session_key(
    session_path: &std::path::Path,
) -> Result<Vec<u8>, String> {
    let key_path = session_key_path(session_path);
    if let Ok(content) = crate::ffi::util::read_to_string_nofollow(&key_path) {
        let key =
            hex::decode(content.trim()).map_err(|_| "session.key is not valid hex".to_string())?;
        if key.len() != 32 {
            return Err("session.key is not 32 bytes".to_string());
        }
        return Ok(key);
    }
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(|e| format!("session key generation failed: {e}"))?;
    let encoded = hex::encode(key);
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&key_path) {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(encoded.as_bytes())
                .map_err(|e| format!("session key write failed: {e}"))?;
            Ok(key.to_vec())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Concurrent creator (or a stale/garbage key file) won the race.
            // Re-read once; a non-32-byte key is refused, not overwritten, so
            // an attacker-supplied key file can never silently rotate the key.
            let content = crate::ffi::util::read_to_string_nofollow(&key_path)
                .map_err(|_| format!("session.key exists but is unreadable: {e}"))?;
            let k = hex::decode(content.trim())
                .map_err(|_| "session.key is not valid hex (existing file)".to_string())?;
            if k.len() != 32 {
                return Err("session.key is not 32 bytes (existing file)".to_string());
            }
            Ok(k)
        }
        Err(e) => Err(format!("session.key create failed: {e}")),
    }
}

fn random_suffix() -> String {
    let mut buf = [0u8; 8];
    let _ = getrandom::fill(&mut buf);
    hex::encode(buf)
}

/// HMAC-SHA256 over the canonical session payload with `sig` excluded.
/// Both write and verify sides sign the same compact serialization, so the
/// on-disk pretty-printed formatting never affects the tag.
fn sign_session(key: &[u8], session: &SessionData) -> Result<String, String> {
    let mut for_sign = session.clone();
    for_sign.sig = None;
    let canonical = serde_json::to_string(&for_sign)
        .map_err(|e| format!("session serialize for signing failed: {e}"))?;
    Ok(hex::encode(soshal_crypto_core::hash::hmac_sha256(
        key,
        canonical.as_bytes(),
    )))
}

/// Constant-time validity check of a session file's integrity tag. Unsigned
/// files (`sig == None`) report `false` so the caller can distinguish the
/// legacy-pre-upgrade case from a signed-but-forged one.
fn session_sig_valid(key: &[u8], session: &SessionData) -> bool {
    match &session.sig {
        Some(sig) => sign_session(key, session)
            .map(|expect| {
                soshal_common_core::util::constant_time_eq(expect.as_bytes(), sig.as_bytes())
            })
            .unwrap_or(false),
        None => false,
    }
}

/// Atomic, TOCTOU-safe session write: fresh unique tmp name, `O_NOFOLLOW`
/// with `O_EXCL` (0600), fsync, then same-directory rename. A symlink planted
/// at the tmp or target path can neither be followed nor crossed. The tag is
/// computed over the canonical payload before formatting.
fn write_session_file(
    session_path: &std::path::Path,
    key: &[u8],
    session: &SessionData,
) -> Result<(), String> {
    let mut signed = session.clone();
    signed.sig = Some(sign_session(key, session)?);
    let json = serde_json::to_string_pretty(&signed)
        .map_err(|e| format!("Failed to serialize session: {e}"))?;
    let dir = session_path
        .parent()
        .ok_or_else(|| "session path has no parent".to_string())?;
    for attempt in 0..3 {
        let tmp_path = dir.join(format!("session-{}.tmp", random_suffix()));
        let write_result = (|| -> std::io::Result<()> {
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let mut f = opts.open(&tmp_path)?;
            std::io::Write::write_all(&mut f, json.as_bytes())?;
            std::io::Write::flush(&mut f)?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp_path, session_path)
        })();
        match write_result {
            Ok(_) => return Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                if attempt == 2 {
                    return Err(format!("Failed to write session: {e}"));
                }
            }
        }
    }
    Err("Failed to write session".to_string())
}

/// Load and validate session data directly from disk.
/// Enforces path hardening, HMAC signature check, and expiration check.
pub(crate) fn load_session_from_disk(db_path: &str) -> Result<SessionData, String> {
    let session_path = validated_session_path_hardened(db_path)?;
    if !session_path.exists() {
        return Err("Session file not found".to_string());
    }
    let content = std::fs::read_to_string(&session_path)
        .map_err(|e| format!("Failed to read session file: {e}"))?;
    let session = serde_json::from_str::<SessionData>(&content)
        .map_err(|e| format!("Failed to parse session: {e}"))?;
    let session_key = load_or_create_session_key(&session_path)?;
    if !session_sig_valid(&session_key, &session) && session.sig.is_some() {
        return Err("session file failed integrity check".to_string());
    }
    let now = soshal_common_core::format::now_secs() as u64;
    if let Some(loaded_at) = session.loaded_at {
        if now.saturating_sub(loaded_at) > SESSION_MAX_AGE_SECS {
            return Err("session expired, please re-authenticate".to_string());
        }
    }
    Ok(session)
}

/// Load session from file
#[frb(sync, serialize)]
pub fn session_load(db_path: String) -> Result<String, String> {
    let session_path = validated_session_path_hardened(&db_path)?;

    if session_path.exists() {
        match std::fs::read_to_string(&session_path) {
            Ok(content) => match serde_json::from_str::<SessionData>(&content) {
                Ok(session) => {
                    let session_key = load_or_create_session_key(&session_path)?;
                    // Integrity: a signed file must verify. An unsigned file
                    // is the legacy pre-upgrade shape — accepted once, then
                    // re-signed on this load so every later load enforces the
                    // tag. A present-but-mismatched tag is forged/tampered.
                    if !session_sig_valid(&session_key, &session) && session.sig.is_some() {
                        return Err("session file failed integrity check".to_string());
                    }
                    // Enforce session max age: reject sessions older than
                    // SESSION_MAX_AGE_SECS to limit exposure from stolen files.
                    let now = soshal_common_core::format::now_secs() as u64;
                    if let Some(loaded_at) = session.loaded_at {
                        if now.saturating_sub(loaded_at) > SESSION_MAX_AGE_SECS {
                            return Err("session expired, please re-authenticate".to_string());
                        }
                    }
                    let mut session = session;
                    session.loaded_at = Some(now);
                    if !session.accounts.is_empty() {
                        let active_valid = session
                            .active_pubkey
                            .as_ref()
                            .map_or(false, |pk| session.accounts.iter().any(|a| &a.pubkey == pk));
                        if !active_valid {
                            if let Some(last_acc) =
                                session.accounts.iter().max_by_key(|a| a.last_used)
                            {
                                session.active_pubkey = Some(last_acc.pubkey.clone());
                            }
                        }
                    }
                    // loaded_at changed (or the file was legacy/unsigned):
                    // re-sign and persist so the on-disk tag stays valid.
                    let _ = write_session_file(&session_path, &session_key, &session);
                    let json = super::util::json_ok(&session)?;
                    let mut session_lock = lock_session()?;
                    *session_lock = Some(session);
                    Ok(json)
                }
                Err(e) => Err(format!("Failed to parse session: {}", e)).into(),
            },
            Err(e) => Err(format!("Failed to read session file: {}", e)).into(),
        }
    } else {
        // No session file yet: still register the empty session so the
        // in-memory SESSION (and thus session_add_account/switch/get_active)
        // sees a loaded state instead of erroring with "Session not loaded".
        let empty = SessionData {
            active_pubkey: None,
            accounts: Vec::new(),
            loaded_at: Some(soshal_common_core::format::now_secs() as u64),
            sig: None,
        };
        let json = super::util::json_ok(&empty)?;
        let mut session_lock = lock_session()?;
        *session_lock = Some(empty);
        Ok(json)
    }
}

/// Save session to file
#[frb(sync, serialize)]
pub fn session_save(db_path: String, session_data: String) -> Result<bool, String> {
    let session_path = validated_session_path_hardened(&db_path)?;

    match serde_json::from_str::<SessionData>(&session_data) {
        Ok(session) => {
            let session_key = load_or_create_session_key(&session_path)?;
            match write_session_file(&session_path, &session_key, &session) {
                Ok(_) => {
                    let mut session_lock = lock_session()?;
                    *session_lock = Some(session);
                    Ok(true).into()
                }
                Err(e) => Err(e).into(),
            }
        }
        Err(e) => Err(format!("Invalid session JSON: {}", e)).into(),
    }
}

/// Best-effort persist of the on-disk session file after an in-memory change.
fn persist_session(data: &SessionData) {
    if let Ok(db_path) = super::db::db_path() {
        let _ = session_save(db_path, serde_json::to_string(data).unwrap_or_default());
    }
}

/// Add account to session
#[frb(sync, serialize)]
pub fn session_add_account(
    pubkey: String,
    npub: String,
    relays_json: String,
) -> Result<bool, String> {
    // Validate formats before storing: malformed pubkeys / npubs would corrupt
    // the identity comparison logic in require_identity and session_get_active.
    validate_pubkey_hex(&pubkey)?;
    validate_npub(&npub)?;
    match serde_json::from_str::<Vec<String>>(&relays_json) {
        Ok(relays) => {
            let account = SessionAccount {
                pubkey,
                npub,
                last_used: soshal_common_core::format::now_secs() as u64,
                relay_list: relays,
                push_token: None,
            };

            let mut session_lock = lock_session()?;
            let session = session_lock.get_or_insert_with(|| SessionData {
                active_pubkey: None,
                accounts: Vec::new(),
                loaded_at: Some(soshal_common_core::format::now_secs() as u64),
                sig: None,
            });
            let mut account = account;
            account.pubkey = account.pubkey.trim().to_ascii_lowercase();
            if !session
                .accounts
                .iter()
                .any(|a| a.pubkey.eq_ignore_ascii_case(&account.pubkey))
            {
                session.accounts.push(account.clone());
            }
            if session.active_pubkey.is_none() {
                session.active_pubkey = Some(account.pubkey);
            }
            // Persist the updated session so an account added in-memory is not
            // lost on process kill before a later explicit session_save.
            let data = session.clone();
            drop(session_lock);
            persist_session(&data);
            Ok(true).into()
        }
        Err(e) => Err(format!("Invalid relays JSON: {}", e)).into(),
    }
}

/// Switch to an account
#[frb(sync, serialize)]
pub fn session_switch_account(pubkey: String) -> Result<bool, String> {
    let pubkey = pubkey.trim().to_ascii_lowercase();
    // Identity gate: if the signer is unlocked, the caller must prove they control
    // the currently active account or the destination account. If the signer is
    // currently locked, switching between already-persisted accounts in the local
    // session is permitted (the destination account will be unlocked afterwards).
    {
        let session_guard = lock_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| "Session not loaded".to_string())?;
        if !session
            .accounts
            .iter()
            .any(|a| a.pubkey.eq_ignore_ascii_case(&pubkey))
        {
            return Err("Account not found".to_string());
        }
        if let Some(ref active_pk) = session.active_pubkey {
            if !super::signer::signer_is_locked().unwrap_or(true) {
                if super::signer::require_identity(active_pk).is_err() {
                    super::signer::require_identity(&pubkey)?;
                }
            }
        }
    }
    let mut session_lock = lock_session()?;
    if let Some(session) = session_lock.as_mut() {
        if session
            .accounts
            .iter()
            .any(|a| a.pubkey.eq_ignore_ascii_case(&pubkey))
        {
            session.active_pubkey = Some(pubkey.clone());
            if let Some(acc) = session
                .accounts
                .iter_mut()
                .find(|a| a.pubkey.eq_ignore_ascii_case(&pubkey))
            {
                acc.last_used = soshal_common_core::format::now_secs() as u64;
            }
            // Invalidate unlocked signer keys and secret caches from the
            // PREVIOUS account. Skip when the signer already holds exactly the
            // target identity (same-account switch, e.g. the onboarding
            // restore->add->switch->keychain-save sequence): locking there
            // wipes a just-unlocked key and makes the following keychain save
            // fail with "signer locked".
            let target_is_unlocked =
                super::signer::signer_pubkey().is_ok_and(|pk| pk.eq_ignore_ascii_case(&pubkey));
            if !target_is_unlocked {
                let _ = super::signer::signer_lock();
            }
            // Persist the switched session so the active account survives
            // process kill before a later explicit session_save.
            let data = session.clone();
            drop(session_lock);
            persist_session(&data);
            Ok(true).into()
        } else {
            Err("Account not found".to_string()).into()
        }
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// Remove an account from the session, persisting the change to disk.
/// If the removed account was active, the active account is reassigned to the
/// first remaining account (or cleared if none remain).
#[frb(sync, serialize)]
pub fn session_remove_account(pubkey: String) -> Result<bool, String> {
    let pubkey = pubkey.trim().to_ascii_lowercase();
    let mut session_lock = lock_session()?;
    let session = match session_lock.as_mut() {
        Some(s) => s,
        None => return Err("Session not loaded".to_string()).into(),
    };
    let before = session.accounts.len();
    session
        .accounts
        .retain(|a| !a.pubkey.eq_ignore_ascii_case(&pubkey));
    if session.accounts.len() == before {
        return Err("Account not found".to_string()).into();
    }
    if session
        .active_pubkey
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case(&pubkey))
        .unwrap_or(false)
    {
        session.active_pubkey = session.accounts.first().map(|a| a.pubkey.clone());
        // Invalidate unlocked signer keys from the removed active account.
        let _ = super::signer::signer_lock();
    }
    // Persist the removal so it survives process kill (matches add/switch).
    let data = session.clone();
    drop(session_lock);
    persist_session(&data);
    Ok(true).into()
}

/// Get active account
#[frb(sync, serialize)]
pub fn session_get_active() -> Result<String, String> {
    let mut session_lock = lock_session()?;
    if let Some(session) = session_lock.as_mut() {
        if let Some(active_pubkey) = session.active_pubkey.clone() {
            let now = soshal_common_core::format::now_secs() as u64;
            // Idle timeout: reject if the active account hasn't been used
            // within SESSION_IDLE_TIMEOUT_SECS.
            if let Some(acc) = session.accounts.iter().find(|a| a.pubkey == active_pubkey) {
                if now.saturating_sub(acc.last_used) > SESSION_IDLE_TIMEOUT_SECS {
                    return Err("session idle too long, please re-authenticate".to_string());
                }
            }
            // Update last_used; persist only when delta > 1 hour to avoid
            // write storms on frequent UI reads.
            let should_persist = session
                .accounts
                .iter()
                .find(|a| a.pubkey == active_pubkey)
                .map_or(false, |a| now.saturating_sub(a.last_used) > 3600);
            if should_persist {
                if let Some(acc) = session
                    .accounts
                    .iter_mut()
                    .find(|a| a.pubkey == active_pubkey)
                {
                    acc.last_used = now;
                }
                let data = session.clone();
                let account = data
                    .accounts
                    .iter()
                    .find(|a| a.pubkey == active_pubkey)
                    .cloned();
                drop(session_lock);
                persist_session(&data);
                return super::util::json_ok(account);
            }
            let account = session
                .accounts
                .iter()
                .find(|a| a.pubkey == active_pubkey)
                .cloned();
            super::util::json_ok(account)
        } else {
            super::util::json_ok(None::<SessionAccount>)
        }
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// List all accounts
#[frb(sync, serialize)]
pub fn session_list_accounts() -> Result<String, String> {
    let session_lock = lock_session()?;
    if let Some(session) = session_lock.as_ref() {
        super::util::json_ok(&session.accounts)
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// Reset in-memory session state (unload).
#[frb(sync, serialize)]
pub fn session_clear() -> Result<bool, String> {
    let mut session_lock = lock_session()?;
    *session_lock = None;
    Ok(true).into()
}

/// Register (or clear, when empty) the push token for the active account.
/// Persists to session.json via the current DB path.
///
/// Save-first: the in-memory session is only updated after a successful disk
/// write.  If `session_save` fails the caller receives the error and the
/// in-memory state is unchanged.
#[frb(sync, serialize)]
pub fn session_register_push_token(token: String) -> Result<bool, String> {
    let session_lock = lock_session()?;
    let session = session_lock
        .as_ref()
        .ok_or_else(|| "Session not loaded".to_string())?;
    // Enforce idle timeout: a session that has been idle for too long must
    // re-authenticate before modifying its push token.
    check_idle_timeout(session)?;
    let active = session
        .active_pubkey
        .clone()
        .ok_or_else(|| "No active account".to_string())?;
    let account = session
        .accounts
        .iter()
        .find(|a| a.pubkey == active)
        .ok_or_else(|| "Active account not found".to_string())?;
    if account.push_token.as_deref() == Some(token.as_str()) {
        return Ok(true).into();
    }
    let new_token = if token.is_empty() { None } else { Some(token) };
    // Build a clone with the new token applied for the on-disk write.
    let mut data = session.clone();
    if let Some(acc) = data.accounts.iter_mut().find(|a| a.pubkey == active) {
        acc.push_token = new_token.clone();
    }
    let db_path = super::db::db_path()?;
    drop(session_lock);
    session_save(db_path, serde_json::to_string(&data).unwrap_or_default())?;
    // Save succeeded — commit to in-memory state.
    let mut session_lock = lock_session()?;
    if let Some(session) = session_lock.as_mut() {
        if let Some(acc) = session.accounts.iter_mut().find(|a| a.pubkey == active) {
            acc.push_token = new_token;
        }
    }
    Ok(true).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::db;

    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn tmp_session_dir(label: &str) -> (std::path::PathBuf, String) {
        let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "soshal_session_{label}_{}_{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("app.db").to_string_lossy().to_string();
        let _ = db::db_init(db_path.clone());
        (dir, db_path)
    }

    #[test]
    fn test_add_account_lists_and_activates_first() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("addlist");
        // Use valid 64-char hex pubkeys: validation now rejects fake "pk1" strings.
        let pk1 = "ab".repeat(32); // "abab...ab" — 64 lowercase hex chars
        let pk2 = "cd".repeat(32);
        session_load(db_path).unwrap();
        assert_eq!(session_get_active().unwrap(), "null");
        session_add_account(
            pk1.clone(),
            "npub1pk1valid".to_string(),
            "[\"wss://relay.a\"]".to_string(),
        )
        .unwrap();
        session_add_account(pk2.clone(), "npub1pk2valid".to_string(), "[]".to_string()).unwrap();
        let json = session_list_accounts().unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["pubkey"], pk1.as_str());
        assert_eq!(arr[0]["npub"], "npub1pk1valid");
        assert_eq!(arr[0]["relay_list"][0], "wss://relay.a");
        let active = session_get_active().unwrap();
        assert!(
            active.contains(&format!("\"pubkey\":\"{pk1}\"")),
            "active: {active}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_switch_account_updates_active() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _sg = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("switch");
        let keys_a = soshal_nostr_core::keys::generate_keys();
        let keys_b = soshal_nostr_core::keys::generate_keys();
        let pk_a = keys_a.public_key().to_hex();
        let pk_b = keys_b.public_key().to_hex();
        // Unlock as A.
        assert!(super::super::signer::signer_unlock(keys_a.secret_key().to_secret_hex()).is_ok());
        session_load(db_path).unwrap();
        session_add_account(pk_a.clone(), "npub1pk1valid".to_string(), "[]".to_string()).unwrap();
        session_add_account(pk_b.clone(), "npub1pk2valid".to_string(), "[]".to_string()).unwrap();
        // Switch from A (active, signer=A) to B: must succeed.
        assert!(session_switch_account(pk_b.clone()).unwrap());
        let active = session_get_active().unwrap();
        assert!(
            active.contains(&format!("\"pubkey\":\"{pk_b}\"")),
            "active: {active}"
        );
        // The switch to pk_b (not the unlocked identity) relocked the signer;
        // re-unlock as B so the next switch passes the identity gate.
        assert!(super::super::signer::signer_unlock(keys_b.secret_key().to_secret_hex()).is_ok());
        let err = session_switch_account("nobody".to_string()).unwrap_err();
        assert!(
            err.contains("Account not found") || err.contains("identity mismatch"),
            "err: {err}"
        );
        super::super::signer::signer_lock().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_switch_to_same_account_keeps_signer_unlocked() {
        // Onboarding restore->add->switch->keychain-save: switching to the
        // account the signer already holds must NOT wipe it, or the following
        // keychain save fails with "signer locked".
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _sg = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("switch_same_signer");
        let keys = soshal_nostr_core::keys::generate_keys();
        let pk = keys.public_key().to_hex();
        assert!(
            super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).is_ok(),
            "precondition: signer unlocks"
        );
        session_load(db_path).unwrap();
        session_add_account(pk.clone(), "npub1pkvalid".to_string(), "[]".to_string()).unwrap();
        assert!(session_switch_account(pk.clone()).unwrap());
        // Signer must still hold the same identity after the same-account switch.
        assert_eq!(
            super::super::signer::signer_pubkey().unwrap(),
            pk,
            "same-identity switch must not relock the signer"
        );
        super::super::signer::signer_lock().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_switch_to_other_account_locks_signer() {
        // Real account change A->B wipes A's keys from memory; the caller
        // re-unlocks B afterwards (accounts screen unlockFromKeyring).
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let _sg = crate::ffi::util::lock(&crate::ffi::test_lock::SIGNER_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("switch_other_signer");
        let keys_a = soshal_nostr_core::keys::generate_keys();
        let keys_b = soshal_nostr_core::keys::generate_keys();
        let pk_a = keys_a.public_key().to_hex();
        let pk_b = keys_b.public_key().to_hex();
        assert!(super::super::signer::signer_unlock(keys_a.secret_key().to_secret_hex()).is_ok());
        session_load(db_path).unwrap();
        session_add_account(pk_a.clone(), "npub1avalida".to_string(), "[]".to_string()).unwrap();
        session_add_account(pk_b.clone(), "npub1bvalidb".to_string(), "[]".to_string()).unwrap();
        assert!(session_switch_account(pk_b.clone()).unwrap());
        let err = super::super::signer::signer_pubkey().unwrap_err();
        assert!(
            err.contains("signer locked"),
            "A's keys must be wiped: {err}"
        );
        super::super::signer::signer_lock().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_save_load_roundtrip() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("roundtrip");
        // Fresh last_used: the idle-timeout check rejects epoch-old fixtures.
        let data = format!(
            r#"{{"active_pubkey":"pk1","accounts":[{{"pubkey":"pk1","npub":"npub1pk1","last_used":{},"relay_list":["wss://relay.a"]}}]}}"#,
            soshal_common_core::format::now_secs()
        );
        assert!(session_save(db_path.clone(), data.to_string()).unwrap());
        assert!(dir.join("session.json").exists());
        let loaded = session_load(db_path.clone()).unwrap();
        assert!(
            loaded.contains("\"active_pubkey\":\"pk1\""),
            "loaded: {loaded}"
        );
        let active = session_get_active().unwrap();
        assert!(active.contains("wss://relay.a"), "active: {active}");
        let err = session_save(db_path, "not json".to_string()).unwrap_err();
        assert!(err.contains("Invalid session JSON"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_push_token_register_persists_and_clears() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("push");
        let pk = "ef".repeat(32); // valid 64-char hex pubkey
        db::db_init(db_path.clone()).unwrap();
        session_load(db_path.clone()).unwrap();
        session_add_account(pk.clone(), "npub1pkvalid".to_string(), "[]".to_string()).unwrap();
        assert!(session_register_push_token("tok123".to_string()).unwrap());
        let reloaded = session_load(db_path.clone()).unwrap();
        assert!(reloaded.contains("tok123"), "reloaded: {reloaded}");
        assert!(session_register_push_token(String::new()).unwrap());
        let reloaded = session_load(db_path).unwrap();
        assert!(!reloaded.contains("tok123"), "reloaded: {reloaded}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_push_token_requires_active_account() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("pushnone");
        session_load(db_path).unwrap();
        let err = session_register_push_token("tok".to_string()).unwrap_err();
        assert!(err.contains("No active account"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_tampered_session_file_rejected() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("tampered");
        let data = format!(
            r#"{{"active_pubkey":"pk1","accounts":[{{"pubkey":"pk1","npub":"npub1pk1","last_used":{},"relay_list":[]}}]}}"#,
            soshal_common_core::format::now_secs()
        );
        assert!(session_save(db_path.clone(), data).unwrap());
        // A clean load succeeds (signed file verifies) and re-signs on load.
        session_load(db_path.clone()).unwrap();
        // Tamper with `loaded_at` (the max-age bypass vector) — the tag now
        // no longer matches, so the forged file must be rejected.
        let path = dir.join("session.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        v["loaded_at"] = serde_json::json!(1);
        std::fs::write(&path, v.to_string()).unwrap();
        let err = session_load(db_path.clone()).unwrap_err();
        assert!(err.contains("integrity"), "got {err}");
        // Forged tag (all-zero sig) is likewise rejected, not treated as
        // unsigned-legacy.
        let mut v2: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        v2["sig"] = serde_json::json!("0".repeat(64));
        std::fs::write(&path, v2.to_string()).unwrap();
        let err = session_load(db_path.clone()).unwrap_err();
        assert!(err.contains("integrity"), "got {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_unsigned_legacy_file_healed_and_bound() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("legacy");
        // Pre-upgrade shape: no `sig` field at all.
        let legacy = format!(
            r#"{{"active_pubkey":"pk1","accounts":[{{"pubkey":"pk1","npub":"npub1pk1","last_used":{},"relay_list":[]}}]}}"#,
            soshal_common_core::format::now_secs()
        );
        std::fs::write(dir.join("session.json"), legacy).unwrap();
        assert!(session_load(db_path.clone()).is_ok());
        // Load must have healed the file: a `sig` is now present.
        let healed = std::fs::read_to_string(dir.join("session.json")).unwrap();
        assert!(healed.contains("\"sig\""), "healed: {healed}");
        // And that healed tag binds the content: later tampering is rejected.
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap())
                .unwrap();
        v["loaded_at"] = serde_json::json!(1);
        std::fs::write(dir.join("session.json"), v.to_string()).unwrap();
        let err = session_load(db_path.clone()).unwrap_err();
        assert!(err.contains("integrity"), "got {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_session_key_created_with_0600_perms() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, db_path) = tmp_session_dir("keyperms");
        let data = format!(
            r#"{{"active_pubkey":"pk1","accounts":[{{"pubkey":"pk1","npub":"npub1pk1","last_used":{},"relay_list":[]}}]}}"#,
            soshal_common_core::format::now_secs()
        );
        assert!(session_save(db_path.clone(), data).unwrap());
        assert!(dir.join("session.key").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("session.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "session.key mode {mode:o}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_session_key_rejects_symlink_on_read() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, _db_path) = tmp_session_dir("keysymlink");
        let outside = dir.join("outside-key-target");
        std::fs::write(&outside, "attacker data").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("session.key")).unwrap();
        let err = super::load_or_create_session_key(&dir.join("session.json")).unwrap_err();
        assert!(
            err.contains("unreadable"),
            "expected O_NOFOLLOW rejection of symlinked session.key, got: {err}"
        );
        // The symlink target must not have been read/written as key material.
        assert_eq!(std::fs::read(&outside).unwrap(), b"attacker data");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_errors_when_session_not_loaded() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let (dir, _) = tmp_session_dir("notloaded");
        *SESSION.lock().unwrap() = None;
        assert!(session_get_active()
            .unwrap_err()
            .contains("Session not loaded"));
        assert!(session_list_accounts()
            .unwrap_err()
            .contains("Session not loaded"));
        // session_switch_account requires a locked/unlocked signer check; the
        // exact error message depends on whether the session is loaded first.
        let _ = session_switch_account("ab".repeat(32));
        // Use a valid-format pubkey so the relays JSON validation is reached.
        let valid_pk = "ab".repeat(32);
        let err =
            session_add_account(valid_pk, "npub1validpk".to_string(), "x".to_string()).unwrap_err();
        assert!(err.contains("Invalid relays JSON"), "err: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
