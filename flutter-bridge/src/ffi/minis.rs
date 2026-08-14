//! Minis FFI module
//! Minis, Musicloud, custom profiles

use flutter_rust_bridge::frb;

#[frb(sync, serialize)]
pub fn minis_fetch() -> Result<Vec<String>, String> {
    Ok(vec![]).into()
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
        assert!(minis_fetch().unwrap().is_empty());
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
