//! Integration tests for soshal-notification-core: notification key
//! validation, content formatting, IDs and aggregation.

use soshal_notification_core::aggregator::{
    aggregate_notifications, aggregate_notifications_json, AggregateInput,
};
use soshal_notification_core::events::{
    format_content, format_content_json, notif_id, notif_id_json,
};
use soshal_notification_core::notif::{is_valid_notification_key, is_valid_notification_key_json};

#[test]
fn notification_key_validation() {
    assert!(is_valid_notification_key("like"));
    assert!(is_valid_notification_key("friend_request-1"));
    assert!(is_valid_notification_key("a1_b2"));
    assert!(!is_valid_notification_key(""));
    assert!(!is_valid_notification_key("with space"));
    assert!(!is_valid_notification_key("slash/name"));
    assert!(!is_valid_notification_key("dot.name"));
    assert!(!is_valid_notification_key("quote\"x"));
    assert_eq!(is_valid_notification_key_json("like"), "true");
    assert_eq!(is_valid_notification_key_json("bad key!"), "false");
}

#[test]
fn format_content_types() {
    assert_eq!(
        format_content("like", "Alice"),
        "Alice reacted to your post"
    );
    assert_eq!(format_content("repost", "Bob"), "Bob reposted your post");
    assert_eq!(format_content("zap", "Carol"), "Carol zapped your post");
    assert_eq!(format_content("follow", "Dave"), "Dave followed you");
    assert_eq!(format_content("mention", "Eve"), "Eve mentioned you");
    assert_eq!(
        format_content("reply", "Frank"),
        "Frank replied to your post"
    );
    assert_eq!(
        format_content("friend_request", "Grace"),
        "Grace sent you a friend request"
    );
    assert_eq!(format_content("message", "Hank"), "Hank sent you a message");
    assert_eq!(
        format_content("group_invite", "Ivy"),
        "Ivy invited you to a group"
    );
    assert_eq!(
        format_content("event_invite", "Jack"),
        "Jack invited you to an event"
    );
    assert_eq!(format_content("report", "Kara"), "Kara submitted a report");
    assert_eq!(format_content("vouch", "Leo"), "Leo vouched for you");
    assert_eq!(
        format_content("poll_end", "The vote"),
        "A poll has ended: The vote"
    );
    assert_eq!(
        format_content("check_in", "Mia"),
        "Mia checked in to an event"
    );
    assert_eq!(
        format_content("dating_match", "Nina"),
        "You matched with Nina"
    );
    assert_eq!(
        format_content("weird_type", "Ozzy"),
        "New notification from Ozzy"
    );
}

#[test]
fn format_content_json_roundtrip() {
    let out = format_content_json(r#"{"type":"like","content":"Alice"}"#);
    assert_eq!(out, "Alice reacted to your post");
    assert_eq!(format_content_json("garbage"), "");
}

#[test]
fn notif_ids() {
    assert_eq!(notif_id("like", "e1", "pk1"), "like:e1:pk1");
    assert_eq!(
        notif_id_json(r#"{"type":"zap","eventId":"e9","fromPubkey":"pk9"}"#),
        "zap:e9:pk9"
    );
    assert_eq!(notif_id_json("garbage"), "");
    assert_ne!(notif_id("like", "e1", "pk1"), notif_id("like", "e2", "pk1"));
}

#[test]
fn aggregate_notifications_kinds() {
    let events = vec![
        nostr_event("e1", "pk1", 7, "like+", vec![["e", "target-1"]]),
        nostr_event("e2", "pk2", 9735, "zap 21 sats", vec![]),
        nostr_event("e3", "pk3", 6, "", vec![["e", "target-2"]]),
        nostr_event("e4", "pk4", 1, "hello", vec![["t", "friend-request"]]),
        nostr_event("e5", "pk5", 1, "ping", vec![["e", "target-3"]]),
        nostr_event("e6", "pk6", 1, "shout", vec![]),
        nostr_event("e7", "pk7", 39003, "", vec![]),
        nostr_event("e8", "pk8", 999, "ignored", vec![]),
        nostr_event(
            "e9",
            "pk9",
            30311,
            "go live",
            vec![["title", "Stream time"]],
        ),
    ];
    let input = AggregateInput {
        events,
        existing_ids: vec![],
        live_stream_kind: 30311,
    };
    let out = aggregate_notifications(input);
    assert_eq!(out.len(), 8);
    assert_eq!(out[0].notif_type, "reaction");
    assert_eq!(out[0].event_id, "target-1");
    assert_eq!(out[1].notif_type, "zap");
    assert_eq!(out[3].notif_type, "friend_request");
    assert_eq!(out[4].notif_type, "reply");
    assert_eq!(out[5].notif_type, "mention");
    assert_eq!(out[6].notif_type, "mention");
    assert_eq!(out[7].notif_type, "live_stream");
    assert_eq!(out[7].content, "Went live: Stream time");
}

#[test]
fn aggregate_dedupes_existing_ids() {
    let events = vec![
        nostr_event("e1", "pk1", 7, "like+", vec![["e", "target-1"]]),
        nostr_event("e2", "pk2", 7, "like+", vec![["e", "target-2"]]),
    ];
    let dup = notif_id("reaction", "e2", "pk2");
    let input = AggregateInput {
        events,
        existing_ids: vec![dup],
        live_stream_kind: 0,
    };
    let out = aggregate_notifications(input);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id, "reaction:e1:pk1");
}

#[test]
fn aggregate_json_wrapper() {
    let input = serde_json::json!({
        "events": [{"id":"e1","pubkey":"pk1","kind":7,"created_at":123.0,"content":"+","tags":[["e","t1"]]}],
        "existing_ids": [],
        "live_stream_kind": 0
    })
    .to_string();
    let out = aggregate_notifications_json(&input);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["type"], "reaction");
    assert_eq!(v[0]["content"], "Liked your post");
    assert_eq!(aggregate_notifications_json("garbage"), "[]");
}

fn nostr_event(
    id: &str,
    pubkey: &str,
    kind: u32,
    content: &str,
    tags: Vec<[&str; 2]>,
) -> soshal_nostr_core::models::NostrEvent {
    soshal_nostr_core::models::NostrEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        content: content.to_string(),
        tags: tags
            .iter()
            .map(|t| vec![t[0].to_string(), t[1].to_string()])
            .collect(),
        created_at: 100.0,
        kind,
    }
}

#[test]
fn format_content_livestream_and_empty_cases() {
    assert_eq!(
        format_content("livestream_start", "Alice"),
        "Alice is now live: Alice"
    );
    let events = vec![
        nostr_event("e1", "pk1", 7, "", vec![]),
        nostr_event("e2", "pk2", 9735, "", vec![]),
        nostr_event("e3", "pk3", 1, "", vec![["e", "target-1"]]),
        nostr_event("e4", "pk4", 30311, "", vec![]),
    ];
    let input = AggregateInput {
        events,
        existing_ids: vec![],
        live_stream_kind: 30311,
    };
    let out = aggregate_notifications(input);
    assert_eq!(out[0].content, "Liked your post");
    assert_eq!(out[1].content, "Sent you a zap");
    assert_eq!(out[2].content, "Replied to your post");
    assert_eq!(out[3].content, "Went live!");
}
