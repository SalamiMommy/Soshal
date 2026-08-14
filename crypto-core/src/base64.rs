//! Base64 encode/decode utility.

use base64::Engine;

/// Encodes a UTF-8 string to base64 (standard encoding).
pub fn base64_encode(s: &str) -> String {
    base64_encode_bytes(s.as_bytes())
}

/// Encodes raw bytes to base64 (standard encoding).
pub fn base64_encode_bytes(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::STANDARD;
    STANDARD.encode(bytes)
}

/// Decodes a base64 string to a UTF-8 string. Returns empty string on failure.
pub fn base64_decode(s: &str) -> String {
    use base64::engine::general_purpose::STANDARD;
    match STANDARD.decode(s.as_bytes()) {
        Ok(bytes) => String::from_utf8(bytes).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Decodes a base64 string to raw bytes. Returns `None` on failure.
pub fn base64_decode_bytes(s: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::STANDARD;
    STANDARD.decode(s.as_bytes()).ok()
}
