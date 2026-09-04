//! Note publishing and tagging helpers for feed-core.

/// Validates note content length (measured in Unicode characters).
pub fn validate_note_content(content: &str) -> Result<(), String> {
    if content.is_empty() || content.chars().count() > 64000 {
        return Err("content must be 1-64000 chars".into());
    }
    Ok(())
}
