//! Pure domain logic for live streaming, stories, and live chat message deduplication.

pub mod events;
pub mod moq;
pub mod video_server;

pub const MAX_LIVE_CHAT_MESSAGES: usize = 500;

/// Merges and deduplicates live chat messages by ID with a sliding-window cap.
pub fn merge_live_chat_messages(
    mut existing: Vec<serde_json::Value>,
    incoming: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    let mut seen_ids: std::collections::HashSet<String> = existing
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();

    for inc in incoming {
        let inc_id = inc["id"].as_str().unwrap_or("");
        if !inc_id.is_empty() && !seen_ids.contains(inc_id) {
            seen_ids.insert(inc_id.to_string());
            existing.push(inc);
        }
    }
    existing.sort_by_key(|m| m["created_at"].as_i64().unwrap_or(0));
    if existing.len() > MAX_LIVE_CHAT_MESSAGES {
        let drain_count = existing.len() - MAX_LIVE_CHAT_MESSAGES;
        existing.drain(0..drain_count);
    }
    existing
}
