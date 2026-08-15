//! Content FFI module
//! Hashtags, mentions, link previews, zstd-dictionary compression

use flutter_rust_bridge::frb;
use soshal_content_core::compress::{compress_json_dict, decompress_json_dict};

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

/// Parse a text blob leniently, capping nesting/depth; JSON in, JSON out.
#[frb(sync, serialize)]
pub fn content_safe_json_parse(text: String) -> Result<String, String> {
    Ok(soshal_content_core::safe_json::safe_json_parse_json(&text))
}

/// Extract video URLs from an imeta-style tags JSON array (JSON out).
#[frb(sync, serialize)]
pub fn content_extract_imeta_video_urls(tags_json: String) -> Result<String, String> {
    Ok(soshal_content_core::linkpreview::imeta::extract_imeta_video_urls_json(&tags_json))
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
