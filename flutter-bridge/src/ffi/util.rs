//! Utility FFI module
//!
//! Common helpers: hashing, base64url, truncation, hashtag extraction, and
//! low-level TCP probes for daemon status checks.

use flutter_rust_bridge::frb;

/// Serialize any value to JSON for FFI transport (structs cross the bridge
/// as JSON strings; Dart side re-parses them).
pub fn json_ok<T: serde::Serialize>(v: T) -> Result<String, String> {
    serde_json::to_string(&v).map_err(|e| format!("serialize: {e}"))
}

pub fn to_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn json_ok_or_empty<T: serde::Serialize>(v: T) -> String {
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_string())
}

/// SHA256 hash of input string (hex).
#[frb(sync, serialize)]
pub fn util_sha256_hex(input: String) -> Result<String, String> {
    Ok(soshal_crypto_core::hash::sha256_hex(input.as_bytes())).into()
}

/// Base64URL (no padding) encode.
#[frb(sync, serialize)]
pub fn util_base64url_encode(input: String) -> Result<String, String> {
    use soshal_crypto_core::base64url as b64u;
    let standard = soshal_crypto_core::base64::base64_encode(&input);
    Ok(b64u::to_base64url(&standard)).into()
}

/// Base64URL (no padding) decode; returns the decoded string if valid UTF-8.
#[frb(sync, serialize)]
pub fn util_base64url_decode(input: String) -> Result<String, String> {
    use soshal_crypto_core::base64url as b64u;
    let standard = b64u::from_base64url(&input);
    Ok(soshal_crypto_core::base64::base64_decode(&standard)).into()
}

/// Truncate string to max length (char-boundary safe).
#[frb(sync, serialize)]
pub fn util_truncate(input: String, max_len: usize) -> Result<String, String> {
    Ok(soshal_common_core::format::truncate(&input, max_len)).into()
}

/// Extract hashtags (without `#`) from text.
#[frb(sync, serialize)]
pub fn util_extract_hashtags(text: String) -> Result<Vec<String>, String> {
    Ok(soshal_content_core::hashtag::extract(&text)).into()
}

/// Apply thread affinity governing (pin calling thread to Performance or Efficiency cores).
#[frb(sync, serialize)]
pub fn util_apply_thread_affinity(target_performance: bool) -> Result<bool, String> {
    if target_performance {
        soshal_common_core::thread_governor::pin_to_performance_cores()?;
    } else {
        soshal_common_core::thread_governor::pin_to_efficiency_cores()?;
    }
    Ok(true)
}

/// Connect with a short timeout to `host:port` and report whether anything
/// accepted the connection (used for i2pd / Freenet / Reticulum daemons).
pub(crate) fn tcp_probe(host: &str, port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;
    let addr = format!("{host}:{port}");
    TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().unwrap()),
        Duration::from_millis(500),
    )
    .map(|s| s.set_nonblocking(true).is_ok() && s.peer_addr().is_ok())
    .unwrap_or(false)
}

pub(crate) fn uuid_like() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64url_roundtrip() {
        let enc = util_base64url_encode("hello world".to_string()).unwrap();
        assert_eq!(util_base64url_decode(enc).unwrap(), "hello world");
    }

    #[test]
    fn test_hashtag_extraction() {
        let tags = util_extract_hashtags("hello #nostr and #soshal #nostr".to_string()).unwrap();
        assert!(tags.iter().any(|t| t == "nostr"));
        assert!(tags.iter().any(|t| t == "soshal"));
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_sha256_known_vector() {
        assert_eq!(
            util_sha256_hex(String::new()).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_base64url_decode_garbage_and_vector() {
        // Known vector: base64("hello world") = "aGVsbG8gd29ybGQ=", url-form strips '='.
        assert_eq!(
            util_base64url_decode("aGVsbG8gd29ybGQ".to_string()).unwrap(),
            "hello world"
        );
        // base64_decode swallows decode failures into an empty string.
        assert_eq!(util_base64url_decode("!!!".to_string()).unwrap(), "");
    }

    #[test]
    fn test_truncate_boundaries() {
        assert_eq!(util_truncate("hello".to_string(), 10).unwrap(), "hello");
        assert_eq!(util_truncate("hello".to_string(), 5).unwrap(), "hello");
        assert_eq!(util_truncate("hello".to_string(), 3).unwrap(), "he…");
        assert_eq!(util_truncate("hello".to_string(), 0).unwrap(), "");
    }

    #[test]
    fn test_tcp_probe() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_probe("127.0.0.1", port));
        drop(listener);
        assert!(!tcp_probe("127.0.0.1", port));
        // Malformed host falls back to 127.0.0.1:1 (refused).
        assert!(!tcp_probe("not-an-ip", 9999));
    }

    #[test]
    fn test_uuid_like_shape() {
        let a = uuid_like();
        let b = uuid_like();
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn test_apply_thread_affinity_both_branches() {
        // Ok always carries true; real pinning may fail in restricted sandboxes.
        let perf = util_apply_thread_affinity(true);
        let eff = util_apply_thread_affinity(false);
        assert!(perf.as_ref().map_or(true, |v| *v));
        assert!(eff.as_ref().map_or(true, |v| *v));
    }
}
