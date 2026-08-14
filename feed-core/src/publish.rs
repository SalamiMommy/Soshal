//! Note publishing and tagging helpers for feed-core.

/// Validates note content length.
pub fn validate_note_content(content: &str) -> Result<(), String> {
    if content.is_empty() || content.len() > 64000 {
        return Err("content must be 1-64000 chars".into());
    }
    Ok(())
}

/// Builds Nostr tags for ephemeral relay signaling pointing to a Freenet contract key.
pub fn build_freenet_ephemeral_tags(freenet_key: &str) -> Vec<Vec<String>> {
    vec![
        vec!["freenet".to_string(), freenet_key.to_string()],
        vec!["ephemeral".to_string(), "true".to_string()],
        vec!["retention".to_string(), "0".to_string()],
    ]
}
