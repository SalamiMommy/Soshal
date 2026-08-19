//! WASI 0.2 WebAssembly Component Model Runtime Host.
//! Embeds sandboxed execution for dynamic community plugins: feed rankers, content filters, and UI theme generators.

use serde::{Deserialize, Serialize};

/// Type of WASI 0.2 component extension.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WasmComponentType {
    FeedRanker,
    ContentFilter,
    ThemeGenerator,
}

/// Metadata and config for an installed Wasm component plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmComponentPlugin {
    pub plugin_id: String,
    pub name: String,
    pub component_type: WasmComponentType,
    pub author_pubkey: String,
    pub binary_bytes: Vec<u8>,
}

/// Result returned from executing a Wasm content filter component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmFilterResult {
    pub allow: bool,
    pub score: f32,
    pub reason: String,
}

/// Host execution manager for Wasm components.
pub struct WasmComponentHost;

impl WasmComponentHost {
    /// Execute a FeedRanker Wasm component to dynamically sort a list of post JSONs.
    pub fn rank_posts(
        plugin: &WasmComponentPlugin,
        _posts_json: Vec<String>,
    ) -> Result<Vec<String>, String> {
        if plugin.component_type != WasmComponentType::FeedRanker {
            return Err("invalid plugin component type for feed ranking".to_string());
        }

        // Honest gate: wasmtime host is on the roadmap; no simulated ranking
        // is performed (previously a pointless len-key sort pretending to rank).
        Err("wasm feed ranking unavailable (roadmap)".to_string())
    }

    /// Execute a ContentFilter Wasm component to evaluate text.
    pub fn filter_content(
        plugin: &WasmComponentPlugin,
        _text: &str,
    ) -> Result<WasmFilterResult, String> {
        if plugin.component_type != WasmComponentType::ContentFilter {
            return Err("invalid plugin component type for content filter".to_string());
        }

        // Honest gate: wasmtime host is on the roadmap; no simulated filtering
        // is performed (previously a hardcoded substring match pretending to
        // be a Wasm component verdict).
        Err("wasm content filtering unavailable (roadmap)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_content_filter_wasm_host_unavailable() {
        let plugin = WasmComponentPlugin {
            plugin_id: "filter_01".to_string(),
            name: "Spam Guard Wasm".to_string(),
            component_type: WasmComponentType::ContentFilter,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d], // \0asm magic bytes
        };

        let err = WasmComponentHost::filter_content(&plugin, "Hello safe text").unwrap_err();
        assert!(err.contains("unavailable"), "err: {err}");
        let err =
            WasmComponentHost::filter_content(&plugin, "bad malicious_phishing link").unwrap_err();
        assert!(err.contains("unavailable"), "err: {err}");
    }

    #[test]
    fn test_rank_posts_wasm_host_unavailable() {
        let plugin = WasmComponentPlugin {
            plugin_id: "ranker_01".to_string(),
            name: "Feed Ranker Wasm".to_string(),
            component_type: WasmComponentType::FeedRanker,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d],
        };

        let err = WasmComponentHost::rank_posts(
            &plugin,
            vec!["a".to_string(), "ccc".to_string(), "bb".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("unavailable"), "err: {err}");
        let err = WasmComponentHost::rank_posts(&plugin, vec![]).unwrap_err();
        assert!(err.contains("unavailable"), "err: {err}");
    }

    #[test]
    fn test_rank_posts_rejects_wrong_component_type() {
        let plugin = WasmComponentPlugin {
            plugin_id: "ranker_02".to_string(),
            name: "Feed Ranker Wasm".to_string(),
            component_type: WasmComponentType::ContentFilter,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d],
        };

        let err = WasmComponentHost::rank_posts(&plugin, vec!["post".to_string()]).unwrap_err();
        assert_eq!(err, "invalid plugin component type for feed ranking");
    }
}
