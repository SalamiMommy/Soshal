use serde::Deserialize;
use soshal_nostr_core::models::find_tag_value;

/// Maps notification type to human-readable text.
pub fn format_content(notif_type: &str, content: &str, tags: &[Vec<String>]) -> String {
    match notif_type {
        "like" | "reaction" => format!("{} reacted to your post", content),
        "repost" => format!("{} reposted your post", content),
        "zap" => format!("{} zapped your post", content),
        "follow" => format!("{} followed you", content),
        "mention" => format!("{} mentioned you", content),
        "reply" => format!("{} replied to your post", content),
        "friend_request" => format!("{} sent you a friend request", content),
        "message" => format!("{} sent you a message", content),
        "group_invite" => format!("{} invited you to a group", content),
        "event_invite" => format!("{} invited you to an event", content),
        "report" => format!("{} submitted a report", content),
        "vouch" => format!("{} vouched for you", content),
        "poll_end" => format!("A poll has ended: {}", content),
        "livestream_start" => {
            let title = find_tag_value(tags, "title").unwrap_or("");
            if title.is_empty() {
                format!("{} went live", content)
            } else {
                format!("{} is now live: {}", content, title)
            }
        }
        "check_in" => format!("{} checked in to an event", content),
        "dating_match" => format!("You matched with {}", content),
        _ => format!("New notification from {}", content),
    }
}

/// JSON wrapper: accepts `{"type":"...","content":"..."}`, returns formatted string.
pub fn format_content_json(input_json: &str) -> String {
    #[derive(Deserialize)]
    struct Input {
        #[serde(rename = "type")]
        notif_type: String,
        content: String,
    }
    serde_json::from_str::<Input>(input_json)
        .map(|i| format_content(&i.notif_type, &i.content, &[]))
        .unwrap_or_default()
}

/// Generates a deterministic notification ID from type, event ID, and sender pubkey.
pub fn notif_id(notif_type: &str, event_id: &str, from_pubkey: &str) -> String {
    format!("{}:{}:{}", notif_type, event_id, from_pubkey)
}

/// JSON wrapper: accepts `{"type":"...","eventId":"...","fromPubkey":"..."}`, returns ID.
pub fn notif_id_json(input_json: &str) -> String {
    #[derive(Deserialize)]
    struct Input {
        #[serde(rename = "type")]
        notif_type: String,
        #[serde(rename = "eventId")]
        event_id: String,
        #[serde(rename = "fromPubkey")]
        from_pubkey: String,
    }
    serde_json::from_str::<Input>(input_json)
        .map(|i| notif_id(&i.notif_type, &i.event_id, &i.from_pubkey))
        .unwrap_or_default()
}
