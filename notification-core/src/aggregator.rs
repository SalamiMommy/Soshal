use serde::{Deserialize, Serialize};
use soshal_content_core::sanitize::sanitize_notif_content;
use soshal_nostr_core::models::{find_tag_value, find_tag_values_map};

use crate::events::notif_id;
use crate::json_util::{json_in, json_out};

// ─── Types ─────────────────────────────────────────────────────────────

pub type RawEvent = soshal_nostr_core::models::NostrEvent;

#[derive(Deserialize)]
pub struct AggregateInput {
    pub events: Vec<RawEvent>,
    pub existing_ids: Vec<String>,
    pub live_stream_kind: u32,
}

#[derive(Serialize)]
pub struct NotificationOutput {
    pub id: String,
    #[serde(rename = "type")]
    pub notif_type: String,
    pub event_id: String,
    pub from_pubkey: String,
    pub content: String,
    pub created_at: u64,
}

// ─── Helper ────────────────────────────────────────────────────────────

/// Format notification content text given type and raw event content.
fn format_notification_content(notif_type: &str, content: &str, tags: &[Vec<String>]) -> String {
    match notif_type {
        "reaction" => {
            let trimmed = content.trim();
            if trimmed.is_empty() || trimmed == "+" {
                "Liked your post".to_string()
            } else {
                let safe = sanitize_notif_content(trimmed, 140);
                if safe.is_empty() || safe == "+" {
                    "Liked your post".to_string()
                } else {
                    safe
                }
            }
        }
        "zap" => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                "Sent you a zap".to_string()
            } else {
                let safe = sanitize_notif_content(trimmed, 140);
                if safe.is_empty() {
                    "Sent you a zap".to_string()
                } else {
                    safe
                }
            }
        }
        "reply" => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                "Replied to your post".to_string()
            } else {
                let safe = sanitize_notif_content(trimmed, 140);
                if safe.is_empty() {
                    "Replied to your post".to_string()
                } else {
                    safe
                }
            }
        }
        "follow" => "Started following you".to_string(),
        "repost" => "Reposted your post".to_string(),
        "mention" => "Mentioned you in a post".to_string(),
        "friend_request" => "Sent you a friend request".to_string(),
        "live_stream" => {
            let title = find_tag_value(tags, "title").unwrap_or("");
            if title.is_empty() {
                "Went live!".to_string()
            } else {
                format!("Went live: {}", title)
            }
        }
        _ => sanitize_notif_content(content, 140),
    }
}

// ─── Core logic ────────────────────────────────────────────────────────

pub fn aggregate_notifications(input: AggregateInput) -> Vec<NotificationOutput> {
    let mut out: Vec<NotificationOutput> = Vec::with_capacity(input.events.len());
    let live_stream_kind = input.live_stream_kind;

    let mut seen_ids: std::collections::HashSet<String> = input.existing_ids.into_iter().collect();

    for ev in &input.events {
        let [t_tag, e_tag] = find_tag_values_map(&ev.tags, ["t", "e"]);
        let notif_type: &str = match ev.kind {
            7 => "reaction",
            9735 => "zap",
            6 => "repost",
            k if live_stream_kind != 0 && k == live_stream_kind => "live_stream",
            _ => {
                if ev.kind == soshal_common_core::consts::KIND_TEXT_NOTE as u32 {
                    if t_tag == Some("friend-request") {
                        "friend_request"
                    } else if e_tag.is_some() {
                        "reply"
                    } else {
                        "mention"
                    }
                } else if ev.kind == soshal_common_core::consts::KIND_MENTION as u32 {
                    "mention"
                } else {
                    continue;
                }
            }
        };

        let id = notif_id(notif_type, &ev.id, &ev.pubkey);
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        let event_id = match e_tag {
            Some(e) => e.to_string(),
            None => ev.id.clone(),
        };
        let content = format_notification_content(notif_type, &ev.content, &ev.tags);
        let created_at = if ev.created_at.is_finite() && ev.created_at >= 0.0 {
            ev.created_at as u64
        } else {
            0
        };

        out.push(NotificationOutput {
            id,
            notif_type: notif_type.to_string(),
            event_id,
            from_pubkey: ev.pubkey.clone(),
            content,
            created_at,
        });
    }

    out
}

/// JSON wrapper: accepts serialized `AggregateInput`, returns serialized `Vec<NotificationOutput>`.
pub fn aggregate_notifications_json(input_json: &str) -> String {
    let input = json_in(
        input_json,
        AggregateInput {
            events: vec![],
            existing_ids: vec![],
            live_stream_kind: 0,
        },
    );
    json_out(&aggregate_notifications(input), "[]")
}
