//! Social relation entries: vouch / guestbook / review event mapping and
//! presence validation.

use soshal_nostr_core::models::NostrEvent;

/// Maps a vouch (31989) / guestbook (31925) event to its wire JSON.
pub fn relation_entry_from_event(ev: &NostrEvent) -> serde_json::Value {
    serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "content": ev.content,
        "created_at": ev.created_at as u64,
    })
}
