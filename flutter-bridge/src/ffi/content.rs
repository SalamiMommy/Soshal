//! Content FFI module
//! Hashtags, mentions, link previews, zstd-dictionary compression

use flutter_rust_bridge::frb;
use soshal_content_core::compress::{compress_json_dict, decompress_json_dict};
use soshal_content_core::custom_profile;

/// Parse + strictly validate a custom profile (kind 30085) payload.
/// Returns canonical JSON or an error describing the violation.
#[frb(sync, serialize)]
pub fn content_custom_profile_validate(profile_json: String) -> Result<String, String> {
    custom_profile::parse_and_validate(&profile_json).into()
}

/// Default node JSON for a node type (schema authority moved out of Dart).
#[frb(sync, serialize)]
pub fn content_custom_profile_default_node(
    node_type: String,
    index: u64,
) -> Result<String, String> {
    custom_profile::default_node(&node_type, index).into()
}

/// The 12 supported node types as JSON: [{type,label,icon}].
#[frb(sync, serialize)]
pub fn content_custom_profile_node_types() -> Result<String, String> {
    let types = custom_profile::node_types();
    serde_json::to_string(&types)
        .map_err(super::util::to_err)
        .into()
}

/// Empty default profile JSON: {"themeId":"default","nodes":[]}.
#[frb(sync, serialize)]
pub fn content_custom_profile_default_profile() -> Result<String, String> {
    Ok(custom_profile::default_profile()).into()
}

#[cfg(test)]
mod custom_profile_tests {
    use super::*;

    #[test]
    fn test_custom_profile_validate_roundtrip() {
        let ok = content_custom_profile_validate(
            r#"{"themeId":"default","nodes":[{"id":"w1","type":"container","styles":{},"position":{"row":0,"column":0,"order":0},"properties":{}}]}"#
                .to_string(),
        )
        .unwrap();
        assert!(ok.contains("\"type\":\"container\""));
        assert!(
            content_custom_profile_validate(
                r#"{"themeId":"default","nodes":[{"id":"w1","type":"evil","styles":{},"position":{},"properties":{}}]}"#
                    .to_string()
            )
            .is_err()
        );
    }

    #[test]
    fn test_custom_profile_default_node() {
        let node = content_custom_profile_default_node("text_block".to_string(), 2).unwrap();
        assert!(node.contains("\"type\":\"text_block\""));
        assert!(node.contains("\"order\":2"));
        assert!(content_custom_profile_default_node("nope".to_string(), 0).is_err());
    }

    #[test]
    fn test_custom_profile_node_types_json() {
        let json = content_custom_profile_node_types().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 12);
    }

    #[test]
    fn test_custom_profile_default_profile() {
        let json = content_custom_profile_default_profile().unwrap();
        assert!(json.contains("\"themeId\":\"default\""));
    }
}

/// Compress JSON with the bundled zstd dictionary (feed payloads).
#[frb(sync, serialize)]
pub fn content_compress_json_dict(data: String) -> Result<String, String> {
    Ok(compress_json_dict(&data)).into()
}

/// Decompress a zstd-dict payload; transparently falls back to deflate.
#[frb(sync, serialize)]
pub fn content_decompress_json_dict(encoded: String) -> Result<String, String> {
    Ok(decompress_json_dict(&encoded)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let data =
            r#"{"kind":1,"content":"hello soshal hello soshal","tags":[["t","p2p"]]}"#.to_string();
        let encoded = content_compress_json_dict(data.clone()).unwrap();
        assert!(!encoded.is_empty());
        assert_eq!(content_decompress_json_dict(encoded).unwrap(), data);
    }

    #[test]
    fn test_decompress_garbage_returns_empty() {
        assert_eq!(
            content_decompress_json_dict("!!!not-base64-@@@".to_string()).unwrap(),
            ""
        );
    }

    #[test]
    fn test_extract_hashtags() {
        assert_eq!(
            super::super::util::util_extract_hashtags("hello #world and #rust".to_string())
                .unwrap(),
            vec!["world".to_string(), "rust".to_string()]
        );
        assert!(
            super::super::util::util_extract_hashtags("no tags here".to_string())
                .unwrap()
                .is_empty()
        );
    }
}
