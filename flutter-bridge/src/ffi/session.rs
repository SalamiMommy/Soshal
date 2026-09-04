//! Session FFI module
//! Multi-account, keychain, pin/biometrics

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Session account entry
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionAccount {
    pub pubkey: String,
    pub npub: String,
    pub last_used: u64,
    pub relay_list: Vec<String>,
    #[serde(default)]
    pub push_token: Option<String>,
}

/// Session file structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionData {
    pub active_pubkey: Option<String>,
    pub accounts: Vec<SessionAccount>,
}

lazy_static::lazy_static! {
    static ref SESSION: Mutex<Option<SessionData>> = Mutex::new(None);
}

fn lock_session() -> Result<std::sync::MutexGuard<'static, Option<SessionData>>, String> {
    SESSION
        .lock()
        .map_err(|e| format!("session lock poisoned: {e}"))
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

/// Load session from file
#[frb(sync, serialize)]
pub fn session_load(db_path: String) -> Result<String, String> {
    let session_path = validated_session_path(&db_path)?;

    if session_path.exists() {
        match std::fs::read_to_string(&session_path) {
            Ok(content) => match serde_json::from_str::<SessionData>(&content) {
                Ok(session) => {
                    let mut session_lock = lock_session()?;
                    *session_lock = Some(session.clone());
                    super::util::json_ok(session)
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
        };
        let mut session_lock = lock_session()?;
        *session_lock = Some(empty.clone());
        super::util::json_ok(empty)
    }
}

/// Save session to file
#[frb(sync, serialize)]
pub fn session_save(db_path: String, session_data: String) -> Result<bool, String> {
    let session_path = validated_session_path(&db_path)?;

    match serde_json::from_str::<SessionData>(&session_data) {
        Ok(session) => match serde_json::to_string_pretty(&session) {
            Ok(json) => match std::fs::write(&session_path, json) {
                Ok(_) => {
                    let mut session_lock = lock_session()?;
                    *session_lock = Some(session);
                    Ok(true).into()
                }
                Err(e) => Err(format!("Failed to write session: {}", e)).into(),
            },
            Err(e) => Err(format!("Failed to serialize session: {}", e)).into(),
        },
        Err(e) => Err(format!("Invalid session JSON: {}", e)).into(),
    }
}

/// Add account to session
#[frb(sync, serialize)]
pub fn session_add_account(
    pubkey: String,
    npub: String,
    relays_json: String,
) -> Result<bool, String> {
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
            });
            session.accounts.push(account.clone());
            if session.active_pubkey.is_none() {
                session.active_pubkey = Some(account.pubkey);
            }
            Ok(true).into()
        }
        Err(e) => Err(format!("Invalid relays JSON: {}", e)).into(),
    }
}

/// Switch to an account
#[frb(sync, serialize)]
pub fn session_switch_account(pubkey: String) -> Result<bool, String> {
    let mut session_lock = lock_session()?;
    if let Some(session) = session_lock.as_mut() {
        if session.accounts.iter().any(|a| a.pubkey == pubkey) {
            session.active_pubkey = Some(pubkey);
            // Invalidate unlocked signer keys and secret caches from previous account
            let _ = super::signer::signer_lock();
            Ok(true).into()
        } else {
            Err("Account not found".to_string()).into()
        }
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// Get active account
#[frb(sync, serialize)]
pub fn session_get_active() -> Result<String, String> {
    let session_lock = lock_session()?;
    if let Some(session) = session_lock.as_ref() {
        if let Some(active_pubkey) = &session.active_pubkey {
            let account = session
                .accounts
                .iter()
                .find(|a| &a.pubkey == active_pubkey)
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
        super::util::json_ok(session.accounts.clone())
    } else {
        Err("Session not loaded".to_string()).into()
    }
}

/// Register (or clear, when empty) the push token for the active account.
/// Persists to session.json via the current DB path.
#[frb(sync, serialize)]
pub fn session_register_push_token(token: String) -> Result<bool, String> {
    let mut session_lock = lock_session()?;
    let session = session_lock
        .as_mut()
        .ok_or_else(|| "Session not loaded".to_string())?;
    let active = session
        .active_pubkey
        .clone()
        .ok_or_else(|| "No active account".to_string())?;
    let account = session
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == active)
        .ok_or_else(|| "Active account not found".to_string())?;
    if account.push_token.as_deref() == Some(token.as_str()) {
        return Ok(true).into();
    }
    account.push_token = if token.is_empty() { None } else { Some(token) };
    match super::db::db_path() {
        Ok(db_path) => {
            let data = session.clone();
            drop(session_lock);
            session_save(db_path, serde_json::to_string(&data).unwrap_or_default())
        }
        Err(e) => Err(format!("DB not initialized: {e}")).into(),
    }
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (dir, db_path) = tmp_session_dir("addlist");
        session_load(db_path).unwrap();
        assert_eq!(session_get_active().unwrap(), "null");
        session_add_account(
            "pk1".to_string(),
            "npub1pk1".to_string(),
            "[\"wss://relay.a\"]".to_string(),
        )
        .unwrap();
        session_add_account("pk2".to_string(), "npub1pk2".to_string(), "[]".to_string()).unwrap();
        let json = session_list_accounts().unwrap();
        let arr = serde_json::from_str::<serde_json::Value>(&json)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(arr.len(), 2, "json: {json}");
        assert_eq!(arr[0]["pubkey"], "pk1");
        assert_eq!(arr[0]["npub"], "npub1pk1");
        assert_eq!(arr[0]["relay_list"][0], "wss://relay.a");
        let active = session_get_active().unwrap();
        assert!(active.contains("\"pubkey\":\"pk1\""), "active: {active}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_switch_account_updates_active() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (dir, db_path) = tmp_session_dir("switch");
        session_load(db_path).unwrap();
        session_add_account("pk1".to_string(), "npub1pk1".to_string(), "[]".to_string()).unwrap();
        session_add_account("pk2".to_string(), "npub1pk2".to_string(), "[]".to_string()).unwrap();
        assert!(session_switch_account("pk2".to_string()).unwrap());
        let active = session_get_active().unwrap();
        assert!(active.contains("\"pubkey\":\"pk2\""), "active: {active}");
        let err = session_switch_account("nobody".to_string()).unwrap_err();
        assert!(err.contains("Account not found"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_save_load_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (dir, db_path) = tmp_session_dir("roundtrip");
        let data = r#"{"active_pubkey":"pk1","accounts":[{"pubkey":"pk1","npub":"npub1pk1","last_used":1,"relay_list":["wss://relay.a"]}]}"#;
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (dir, db_path) = tmp_session_dir("push");
        db::db_init(db_path.clone()).unwrap();
        session_load(db_path.clone()).unwrap();
        session_add_account("pk1".to_string(), "npub1pk1".to_string(), "[]".to_string()).unwrap();
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
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (dir, db_path) = tmp_session_dir("pushnone");
        session_load(db_path).unwrap();
        let err = session_register_push_token("tok".to_string()).unwrap_err();
        assert!(err.contains("No active account"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_errors_when_session_not_loaded() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (dir, _) = tmp_session_dir("notloaded");
        *SESSION.lock().unwrap() = None;
        assert!(session_get_active()
            .unwrap_err()
            .contains("Session not loaded"));
        assert!(session_list_accounts()
            .unwrap_err()
            .contains("Session not loaded"));
        assert!(session_switch_account("pk1".to_string())
            .unwrap_err()
            .contains("Session not loaded"));
        let err = session_add_account("pk1".to_string(), "npub1pk1".to_string(), "x".to_string())
            .unwrap_err();
        assert!(err.contains("Invalid relays JSON"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
