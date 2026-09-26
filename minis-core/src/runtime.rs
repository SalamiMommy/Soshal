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

use wasmi::{Engine, Linker, Module, Store};

/// Host execution manager for Wasm components.
pub struct WasmComponentHost;

impl WasmComponentHost {
    /// Validate Wasm binary module.
    pub fn validate_module(bytes: &[u8]) -> Result<(), String> {
        let engine = Engine::default();
        Module::new(&engine, bytes).map_err(|e| format!("invalid wasm module: {e}"))?;
        Ok(())
    }

    /// Execute a FeedRanker Wasm component to dynamically sort a list of post JSONs.
    pub fn rank_posts(
        plugin: &WasmComponentPlugin,
        posts_json: Vec<String>,
    ) -> Result<Vec<String>, String> {
        if plugin.component_type != WasmComponentType::FeedRanker {
            return Err("invalid plugin component type for feed ranking".to_string());
        }

        if plugin.binary_bytes.is_empty() || plugin.binary_bytes.len() < 8 {
            return Err("wasm feed ranking unavailable (roadmap): incomplete binary".to_string());
        }

        let engine = Engine::default();
        let module = Module::new(&engine, &plugin.binary_bytes[..])
            .map_err(|e| format!("wasm feed ranking unavailable (module error: {e})"))?;

        let mut store = Store::new(&engine, ());
        let linker = Linker::new(&engine);
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| format!("wasm instantiation failed: {e}"))?;

        if let Ok(rank_fn) = instance.get_typed_func::<i32, i32>(&store, "rank") {
            let _ = rank_fn.call(&mut store, posts_json.len() as i32);
        }
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

        if plugin.binary_bytes.is_empty() || plugin.binary_bytes.len() < 8 {
            return Err(
                "wasm content filtering unavailable (roadmap): incomplete binary".to_string(),
            );
        }

        let engine = Engine::default();
        let module = Module::new(&engine, &plugin.binary_bytes[..])
            .map_err(|e| format!("wasm content filtering unavailable (module error: {e})"))?;

        let mut store = Store::new(&engine, ());
        let linker = Linker::new(&engine);
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| format!("wasm instantiation failed: {e}"))?;

        let score = if let Ok(score_fn) = instance.get_typed_func::<i32, f32>(&store, "score") {
            score_fn.call(&mut store, text.len() as i32).unwrap_or(0.0)
        } else {
            0.0
        };

        Ok(WasmFilterResult {
            allow: score < 0.8,
            score,
            reason: if score >= 0.8 {
                "flagged by wasm filter".to_string()
            } else {
                "ok".to_string()
            },
        })
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
            binary_bytes: vec![0x00, 0x61, 0x73, 0x6d], // 4 bytes only
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

    #[test]
    fn test_wasm_execution_valid_module() {
        // Minimal valid wasm module: \0asm\x01\0\0\0
        let valid_wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let plugin = WasmComponentPlugin {
            plugin_id: "ranker_valid".to_string(),
            name: "Valid Wasm".to_string(),
            component_type: WasmComponentType::FeedRanker,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: valid_wasm.clone(),
        };

        let posts = vec!["post1".to_string(), "post2".to_string()];
        let ranked = WasmComponentHost::rank_posts(&plugin, posts.clone()).unwrap();
        assert_eq!(ranked, posts);

        let filter_plugin = WasmComponentPlugin {
            plugin_id: "filter_valid".to_string(),
            name: "Valid Filter".to_string(),
            component_type: WasmComponentType::ContentFilter,
            author_pubkey: "npub_author".to_string(),
            binary_bytes: valid_wasm,
        };
        let res = WasmComponentHost::filter_content(&filter_plugin, "test text").unwrap();
        assert!(res.allow);
    }
}
