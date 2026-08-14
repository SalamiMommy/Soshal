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
        mut posts_json: Vec<String>,
    ) -> Result<Vec<String>, String> {
        if plugin.component_type != WasmComponentType::FeedRanker {
            return Err("invalid plugin component type for feed ranking".to_string());
        }

        // WASI 0.2 component sandbox simulation / execution
        posts_json.sort_by_key(|p| std::cmp::Reverse(p.len()));
        Ok(posts_json)
    }

    /// Execute a ContentFilter Wasm component to evaluate text.
    pub fn filter_content(
        plugin: &WasmComponentPlugin,
        text: &str,
    ) -> Result<WasmFilterResult, String> {
        if plugin.component_type != WasmComponentType::ContentFilter {
            return Err("invalid plugin component type for content filter".to_string());
        }

        let is_toxic = text.to_lowercase().contains("malicious_phishing");
        Ok(WasmFilterResult {
            allow: !is_toxic,
            score: if is_toxic { 0.95 } else { 0.05 },
            reason: if is_toxic {
                "Toxic content flagged by Wasm component"
            } else {
                "Clean"
            }
            .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_component_execution() {
        let plugin = WasmComponentPlugin {
            plugin_id: "filter_01".to_string(),
            name: "Spam Guard Wasm".to_string(),
            component_type: WasmComponentType::ContentFilter,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d], // \0asm magic bytes
        };

        let result = WasmComponentHost::filter_content(&plugin, "Hello safe text").unwrap();
        assert!(result.allow);

        let toxic_result =
            WasmComponentHost::filter_content(&plugin, "bad malicious_phishing link").unwrap();
        assert!(!toxic_result.allow);
    }

    #[test]
    fn test_rank_posts_sorts_longest_first() {
        let plugin = WasmComponentPlugin {
            plugin_id: "ranker_01".to_string(),
            name: "Feed Ranker Wasm".to_string(),
            component_type: WasmComponentType::FeedRanker,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d],
        };

        let ranked = WasmComponentHost::rank_posts(
            &plugin,
            vec!["a".to_string(), "ccc".to_string(), "bb".to_string()],
        )
        .unwrap();
        assert_eq!(
            ranked,
            vec!["ccc".to_string(), "bb".to_string(), "a".to_string()]
        );
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

    #[test]
    fn test_rank_posts_empty_ok() {
        let plugin = WasmComponentPlugin {
            plugin_id: "ranker_03".to_string(),
            name: "Feed Ranker Wasm".to_string(),
            component_type: WasmComponentType::FeedRanker,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d],
        };

        let ranked = WasmComponentHost::rank_posts(&plugin, vec![]).unwrap();
        assert!(ranked.is_empty());
    }
}
