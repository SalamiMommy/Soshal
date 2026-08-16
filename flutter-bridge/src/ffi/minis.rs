//! Minis FFI module
//! Minis, Musicloud, custom profiles

use flutter_rust_bridge::frb;

/// Fetch known mini URLs from the local registry: kind-31020 rows ingested
/// by the sync engine (subscription added in sync-core). Newest first.
#[frb(sync, serialize)]
pub fn minis_fetch() -> Result<Vec<String>, String> {
    let json = super::db::db_query_raw(
        "SELECT tags_json FROM posts WHERE kind = 31020 AND is_deleted = 0 \
         ORDER BY created_at DESC LIMIT 200"
            .to_string(),
    )?;
    let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    let mut urls: Vec<String> = Vec::new();
    for row in rows {
        let tags: Vec<Vec<String>> =
            serde_json::from_str(row["tags_json"].as_str().unwrap_or("[]")).unwrap_or_default();
        let Some(url) = tags
            .iter()
            .find(|t| t.first().map(|s| s == "url").unwrap_or(false))
            .and_then(|t| t.get(1))
        else {
            continue;
        };
        if !url.is_empty() && !urls.contains(url) {
            urls.push(url.clone());
        }
    }
    Ok(urls).into()
}

/// Execute a WASI 0.2 Wasm content filter component plugin (runtime itself
/// still simulated in minis-core; wasm bytes must be valid hex).
#[frb(sync, serialize)]
pub fn minis_wasm_execute_filter(
    _plugin_id: String,
    _text: String,
    wasm_bytes_hex: String,
) -> Result<String, String> {
    hex::decode(&wasm_bytes_hex).map_err(|_| "invalid wasm hex".to_string())?;
    Err("wasm runtime unavailable: WASI component host on roadmap, runtime simulated".to_string())
        .into()
}

/// Execute a WASI 0.2 Wasm feed ranker component plugin (runtime itself still
/// simulated in minis-core; wasm bytes must be valid hex).
#[frb(sync, serialize)]
pub fn minis_wasm_rank_feed(
    _plugin_id: String,
    _posts_json: Vec<String>,
    wasm_bytes_hex: String,
) -> Result<Vec<String>, String> {
    hex::decode(&wasm_bytes_hex).map_err(|_| "invalid wasm hex".to_string())?;
    Err("wasm runtime unavailable: WASI component host on roadmap, runtime simulated".to_string())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_returns_empty_list() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = format!(
            "{}/soshal_minis_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            "fetch"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(super::super::db::db_init(path.clone()).is_ok());
        assert!(minis_fetch().unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn fetch_returns_urls_from_mini_rows() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = format!(
            "{}/soshal_minis_{}_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            "rows"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(super::super::db::db_init(path.clone()).is_ok());
        assert!(super::super::db::db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('m1','pk','mini one',31020,100,'[[\"url\",\"https://mini.example/a\"],[\"image\",\"https://img.example/a.png\"]]','pending',0), \
                    ('m2','pk','mini two',31020,200,'[[\"url\",\"https://mini.example/b\"]]','pending',0), \
                    ('n1','pk','not a mini',1,300,'[]','pending',0)"
                .to_string()
        )
        .is_ok());
        let urls = minis_fetch().unwrap();
        assert_eq!(
            urls,
            vec![
                "https://mini.example/b".to_string(),
                "https://mini.example/a".to_string()
            ]
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn wasm_filter_returns_unavailable_error() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = minis_wasm_execute_filter(
            "f1".to_string(),
            "hello world".to_string(),
            "0061736d".to_string(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "wasm runtime unavailable: WASI component host on roadmap, runtime simulated"
        );
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn wasm_filter_rejects_invalid_hex() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err =
            minis_wasm_execute_filter("f1".to_string(), "hello".to_string(), "zz".to_string())
                .unwrap_err();
        assert_eq!(err, "invalid wasm hex");
        super::super::signer::signer_lock().unwrap();
    }

    #[test]
    fn wasm_rank_returns_unavailable_error() {
        let _s = crate::ffi::test_lock::SIGNER_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let keys = soshal_nostr_core::keys::generate_keys();
        super::super::signer::signer_unlock(keys.secret_key().to_secret_hex()).unwrap();
        let err = minis_wasm_rank_feed(
            "r1".to_string(),
            vec!["a".to_string(), "ccc".to_string(), "bb".to_string()],
            "0061736d".to_string(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            "wasm runtime unavailable: WASI component host on roadmap, runtime simulated"
        );
        super::super::signer::signer_lock().unwrap();
    }
}
