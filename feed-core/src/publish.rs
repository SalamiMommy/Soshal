//! Note publishing and tagging helpers for feed-core.

/// Validates note content length (measured in Unicode characters).
///
/// Two-pass guard: byte length is checked first (cheap) to reject large
/// multi-byte payloads that would pass the char count but exceed relay
/// protocol limits (~64 KB); char count is then checked per spec.
pub fn validate_note_content(content: &str) -> Result<(), String> {
    if content.is_empty()
        || content.len() > 65_536 // byte guard — 64-KiB relay limit
        || content.chars().count() > 64_000
    {
        return Err("content must be 1-64000 chars and ≤65536 bytes".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_note_content_empty_rejected() {
        assert!(validate_note_content("").is_err());
    }

    #[test]
    fn test_validate_note_content_normal_ok() {
        assert!(validate_note_content("hello world").is_ok());
    }

    #[test]
    fn test_validate_note_content_char_limit_rejected() {
        // 65 000 ASCII chars — passes byte guard (65 000 bytes ≤ 65 536)
        // but exceeds the 64 000 char limit.
        let long: String = "a".repeat(65_000);
        assert!(validate_note_content(&long).is_err());
    }

    #[test]
    fn test_validate_note_content_byte_guard_fires_before_char_count() {
        // 16 385 four-byte Unicode emoji = 65 540 bytes but only 16 385 chars.
        // Without the byte guard this would pass the char count check (16 385 ≤ 64 000).
        // The byte guard (> 65_536) must catch it.
        let emoji: String = "🔥".repeat(16_385); // each '🔥' is exactly 4 UTF-8 bytes
        assert_eq!(emoji.len(), 65_540);
        assert_eq!(emoji.chars().count(), 16_385);
        assert!(
            validate_note_content(&emoji).is_err(),
            "byte guard must reject >65536-byte content"
        );

        // Exactly at the boundary (65 536 bytes) is allowed (guard uses `>`).
        let at_boundary: String = "🔥".repeat(16_384);
        assert_eq!(at_boundary.len(), 65_536);
        assert!(
            validate_note_content(&at_boundary).is_ok(),
            "65_536 bytes should be at the allowed boundary"
        );
    }

    #[test]
    fn test_validate_note_content_just_under_limits_ok() {
        // 64 000 ASCII chars = 64 000 bytes — both limits satisfied.
        let ok: String = "a".repeat(64_000);
        assert!(validate_note_content(&ok).is_ok());
    }
}
