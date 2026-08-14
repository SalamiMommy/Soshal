//! Size limits for relay-sourced rows stored in SQLite.
//!
//! Relay content is untrusted input: an attacker-promoted relay can push
//! arbitrarily large events, bloating the local DB and feed JSON. These
//! caps are enforced at the repository write chokepoints (no relay data
//! reaches the database otherwise), so oversized events are rejected
//! before they land on disk.

/// Longest accepted single content payload (bytes).
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;

/// Longest accepted serialized event payload — content + tags (bytes).
pub const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;

/// Longest accepted notification row (bytes).
pub const MAX_NOTIFICATION_BYTES: usize = 4 * 1024;

/// True when a row's content or combined serialized size exceeds caps.
pub fn row_too_big(content: &str, tags_json: &str) -> bool {
    if content.len() > MAX_CONTENT_BYTES {
        return true;
    }
    let total = content.len() + tags_json.len();
    total > MAX_BATCH_BYTES
}

/// True when a notification row's payload exceeds its (stricter) cap.
pub fn notification_too_big(content: &str) -> bool {
    content.len() > MAX_NOTIFICATION_BYTES
}
