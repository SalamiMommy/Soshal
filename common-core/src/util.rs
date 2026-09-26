use subtle::ConstantTimeEq;

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub use simdutf8::compat::Utf8Error;

/// SIMD-accelerated UTF-8 validation and conversion.
///
/// Returns a standard `&str` on success or `simdutf8::compat::Utf8Error` on failure.
/// Leverages AVX2/SSE4.2/NEON SIMD vectorization for high-throughput stream validation.
#[inline]
pub fn fast_validate_utf8(bytes: &[u8]) -> Result<&str, Utf8Error> {
    simdutf8::compat::from_utf8(bytes)
}

/// SIMD-accelerated check for whether a byte slice contains valid UTF-8.
#[inline]
pub fn fast_is_valid_utf8(bytes: &[u8]) -> bool {
    simdutf8::basic::from_utf8(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_now_ms() {
        let ms = now_ms();
        let secs = crate::format::now_secs() as u64;
        assert!(ms >= secs * 1000);
        assert!(ms <= (secs + 2) * 1000);
    }

    #[test]
    fn test_fast_validate_utf8() {
        let valid = b"Hello, \xf0\x9f\x8c\x8d! Nostr & Soshal.";
        assert!(fast_is_valid_utf8(valid));
        assert_eq!(
            fast_validate_utf8(valid).unwrap(),
            "Hello, 🌍! Nostr & Soshal."
        );

        let invalid = b"\xff\xfe\xfd";
        assert!(!fast_is_valid_utf8(invalid));
        assert!(fast_validate_utf8(invalid).is_err());

        assert!(fast_is_valid_utf8(b""));
        assert_eq!(fast_validate_utf8(b"").unwrap(), "");
    }
}
