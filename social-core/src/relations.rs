//! Social relation entries: vouch / guestbook / review event mapping and
//! presence validation.

use soshal_nostr_core::models::NostrEvent;

/// Validates a presence status string against the accepted set.
pub fn validate_presence(status: &str) -> Result<(), String> {
    if !["online", "idle", "dnd", "offline"].contains(&status) {
        return Err(format!("invalid presence status: {}", status));
    }
    Ok(())
}

/// Maps a vouch (31989) / guestbook (31925) event to its webview JSON.
pub fn relation_entry_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
    })
}

/// Maps a review (30076) event to its webview JSON, extracting the `rating`
/// single-letter tag when present.
pub fn review_from_event(ev: &NostrEvent) -> serde_json::Value {
    let rating = ev
        .tags
        .iter()
        .find(|t| t.first().map(|s| s.as_str()) == Some("rating"))
        .and_then(|t| t.get(1))
        .and_then(|s| s.parse::<u8>().ok());
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
        "rating": rating,
    })
}
