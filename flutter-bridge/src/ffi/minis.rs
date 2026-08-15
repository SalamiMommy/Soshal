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

/// Execute a WASI 0.2 Wasm content filter component plugin.
#[frb(sync, serialize)]
pub fn minis_wasm_execute_filter(
    plugin_id: String,
    text: String,
    wasm_bytes_hex: String,
) -> Result<String, String> {
    let binary_bytes =
        hex::decode(&wasm_bytes_hex).unwrap_or_else(|_| vec![0x00, 0x61, 0x73, 0x6d]);
    let plugin = soshal_minis_core::runtime::WasmComponentPlugin {
        plugin_id,
        name: "Wasm Filter".to_string(),
        component_type: soshal_minis_core::runtime::WasmComponentType::ContentFilter,
        author_pubkey: "npub_author".to_string(),
        binary_bytes,
    };

    let result = soshal_minis_core::runtime::WasmComponentHost::filter_content(&plugin, &text)?;
    serde_json::to_string(&result)
        .map_err(|e| format!("json encode error: {e}"))
        .into()
}

/// Execute a WASI 0.2 Wasm feed ranker component plugin.
#[frb(sync, serialize)]
pub fn minis_wasm_rank_feed(
    plugin_id: String,
    posts_json: Vec<String>,
    wasm_bytes_hex: String,
) -> Result<Vec<String>, String> {
    let binary_bytes =
        hex::decode(&wasm_bytes_hex).unwrap_or_else(|_| vec![0x00, 0x61, 0x73, 0x6d]);
    let plugin = soshal_minis_core::runtime::WasmComponentPlugin {
        plugin_id,
        name: "Wasm Ranker".to_string(),
        component_type: soshal_minis_core::runtime::WasmComponentType::FeedRanker,
        author_pubkey: "npub_author".to_string(),
        binary_bytes,
    };

    soshal_minis_core::runtime::WasmComponentHost::rank_posts(&plugin, posts_json).into()
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
    fn wasm_filter_flags_malicious_phishing() {
        let clean_json = minis_wasm_execute_filter(
            "f1".to_string(),
            "hello world".to_string(),
            "zz".to_string(),
        )
        .unwrap();
        let clean: serde_json::Value = serde_json::from_str(&clean_json).unwrap();
        assert_eq!(clean["allow"], true);
        assert_eq!(clean["reason"], "Clean");

        let toxic_json = minis_wasm_execute_filter(
            "f1".to_string(),
            "bad malicious_phishing link".to_string(),
            "nothex".to_string(),
        )
        .unwrap();
        let toxic: serde_json::Value = serde_json::from_str(&toxic_json).unwrap();
        assert_eq!(toxic["allow"], false);
        assert_eq!(toxic["score"].as_f64().unwrap(), 0.95);
        assert!(toxic["reason"]
            .as_str()
            .unwrap()
            .contains("Toxic content flagged"));
    }

    #[test]
    fn wasm_rank_sorts_longest_first() {
        let ranked = minis_wasm_rank_feed(
            "r1".to_string(),
            vec!["a".to_string(), "ccc".to_string(), "bb".to_string()],
            "zz".to_string(),
        )
        .unwrap();
        assert_eq!(
            ranked,
            vec!["ccc".to_string(), "bb".to_string(), "a".to_string()]
        );
    }
}
