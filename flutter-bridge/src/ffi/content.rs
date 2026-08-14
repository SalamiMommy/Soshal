//! Content FFI module
//! Hashtags, mentions, link previews, zstd-dictionary compression

use flutter_rust_bridge::frb;
use soshal_content_core::compress::{compress_json_dict, decompress_json_dict};

#[frb(sync, serialize)]
pub fn content_extract_hashtags(_text: String) -> Result<Vec<String>, String> {
    Ok(vec![]).into()
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
