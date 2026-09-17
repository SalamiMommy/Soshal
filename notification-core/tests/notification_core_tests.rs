//! Integration tests for soshal-notification-core: key validation, content
//! formatting, deterministic IDs and event aggregation.

use soshal_notification_core::aggregator::{
    aggregate_notifications, aggregate_notifications_json, AggregateInput, NotificationOutput,
};
use soshal_notification_core::events::{
    format_content, format_content_json, notif_id, notif_id_json,
};
use soshal_notification_core::notif::{is_valid_notification_key, is_valid_notification_key_json};

// ─── notif ──────────────────────────────────────────────────────────────

#[test]
fn notification_key_validation_accepts_alnum_underscore_dash() {
    assert!(is_valid_notification_key("like"));
    assert!(is_valid_notification_key("friend_request-1"));
    assert!(is_valid_notification_key("a1_b2-c3"));
    assert!(is_valid_notification_key("123"));
}

#[test]
fn notification_key_validation_rejects_invalid() {
    assert!(!is_valid_notification_key(""));
    assert!(!is_valid_notification_key("with space"));
    assert!(!is_valid_notification_key("slash/name"));
    assert!(!is_valid_notification_key("dot.name"));
    assert!(!is_valid_notification_key("quote\"x"));
    assert!(!is_valid_notification_key("emoji😀"));
}

#[test]
fn notification_key_json_wrapper() {
    assert_eq!(is_valid_notification_key_json("like"), "true");
    assert_eq!(is_valid_notification_key_json("bad key!"), "false");
    assert_eq!(is_valid_notification_key_json(""), "false");
}

// ─── events ─────────────────────────────────────────────────────────────

#[test]
fn format_content_known_types() {
    assert_eq!(
        format_content("like", "Alice", &[]),
        "Alice reacted to your post"
    );
    assert_eq!(
        format_content("reaction", "Alice", &[]),
        "Alice reacted to your post"
    );
    assert_eq!(
        format_content("repost", "Bob", &[]),
        "Bob reposted your post"
    );
    assert_eq!(
        format_content("zap", "Carol", &[]),
        "Carol zapped your post"
    );
    assert_eq!(format_content("follow", "Dave", &[]), "Dave followed you");
    assert_eq!(format_content("mention", "Eve", &[]), "Eve mentioned you");
    assert_eq!(
        format_content("reply", "Frank", &[]),
        "Frank replied to your post"
    );
    assert_eq!(
        format_content("friend_request", "Grace", &[]),
        "Grace sent you a friend request"
    );
    assert_eq!(
        format_content("message", "Hank", &[]),
        "Hank sent you a message"
    );
    assert_eq!(
        format_content("group_invite", "Ivy", &[]),
        "Ivy invited you to a group"
    );
    assert_eq!(
        format_content("event_invite", "Jack", &[]),
        "Jack invited you to an event"
    );
    assert_eq!(
        format_content("report", "Kara", &[]),
        "Kara submitted a report"
    );
    assert_eq!(format_content("vouch", "Leo", &[]), "Leo vouched for you");
    assert_eq!(
        format_content("poll_end", "The vote", &[]),
        "A poll has ended: The vote"
    );
    assert_eq!(
        format_content(
            "livestream_start",
            "Mia",
            &[vec!["title".to_string(), "Stream Title".to_string()]]
        ),
        "Mia is now live: Stream Title"
    );
    assert_eq!(
        format_content("livestream_start", "Mia", &[]),
        "Mia went live"
    );
    assert_eq!(
        format_content("check_in", "Mia", &[]),
        "Mia checked in to an event"
    );
    assert_eq!(
        format_content("dating_match", "Nina", &[]),
        "You matched with Nina"
    );
}

#[test]
fn format_content_unknown_type_falls_back() {
    assert_eq!(
        format_content("weird_type", "Ozzy", &[]),
        "New notification from Ozzy"
    );
    assert_eq!(
        format_content("", "Ozzy", &[]),
        "New notification from Ozzy"
    );
}

#[test]
fn format_content_json_roundtrip() {
    let out = format_content_json(r#"{"type":"like","content":"Alice"}"#);
    assert_eq!(out, "Alice reacted to your post");
    let out = format_content_json(r#"{"type":"poll_end","content":"Vote now"}"#);
    assert_eq!(out, "A poll has ended: Vote now");
    assert_eq!(format_content_json("garbage"), "");
    assert_eq!(format_content_json(r#"{"type":"like"}"#), "");
    assert_eq!(format_content_json(""), "");
}

#[test]
fn notif_id_is_deterministic_and_distinct() {
    assert_eq!(notif_id("like", "e1", "pk1"), "like:e1:pk1");
    assert_eq!(notif_id("like", "e1", "pk1"), notif_id("like", "e1", "pk1"));
    assert_ne!(notif_id("like", "e1", "pk1"), notif_id("like", "e2", "pk1"));
    assert_ne!(notif_id("like", "e1", "pk1"), notif_id("zap", "e1", "pk1"));
    assert_ne!(notif_id("like", "e1", "pk1"), notif_id("like", "e1", "pk2"));
}

#[test]
fn notif_id_json_roundtrip() {
    assert_eq!(
        notif_id_json(r#"{"type":"zap","eventId":"e9","fromPubkey":"pk9"}"#),
        "zap:e9:pk9"
    );
    assert_eq!(notif_id_json("garbage"), "");
    assert_eq!(notif_id_json(""), "");
    assert_eq!(notif_id_json(r#"{"type":"zap"}"#), "");
}

// ─── aggregator ─────────────────────────────────────────────────────────

fn nostr_event(
    id: &str,
    pubkey: &str,
    kind: u32,
    content: &str,
    tags: &[&[&str]],
) -> soshal_nostr_core::models::NostrEvent {
    soshal_nostr_core::models::NostrEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        content: content.to_string(),
        tags: tags
            .iter()
            .map(|t| t.iter().map(|s| s.to_string()).collect())
            .collect(),
        created_at: 100.0,
        kind,
    }
}

#[test]
fn aggregate_notifications_maps_kinds() {
    let events = vec![
        nostr_event("e1", "pk1", 7, "like+", &[&["e", "target-1"]]),
        nostr_event("e2", "pk2", 9735, "zap 21 sats", &[]),
        nostr_event("e3", "pk3", 6, "", &[&["e", "target-2"]]),
        nostr_event("e4", "pk4", 1, "hello", &[&["t", "friend-request"]]),
        nostr_event("e5", "pk5", 1, "ping", &[&["e", "target-3"]]),
        nostr_event("e6", "pk6", 1, "shout", &[]),
        nostr_event("e7", "pk7", 39003, "", &[]),
        nostr_event("e8", "pk8", 999, "ignored", &[]),
        nostr_event("e9", "pk9", 30311, "go live", &[&["title", "Stream time"]]),
    ];
    let out = aggregate_notifications(AggregateInput {
        events,
        existing_ids: vec![],
        live_stream_kind: 30311,
    });
    assert_eq!(out.len(), 8);
    assert_eq!(out[0].notif_type, "reaction");
    assert_eq!(out[0].event_id, "target-1");
    assert_eq!(out[0].from_pubkey, "pk1");
    assert_eq!(out[1].notif_type, "zap");
    assert_eq!(out[2].notif_type, "repost");
    assert_eq!(out[3].notif_type, "friend_request");
    assert_eq!(out[4].notif_type, "reply");
    assert_eq!(out[5].notif_type, "mention");
    assert_eq!(out[6].notif_type, "mention");
    assert_eq!(out[7].notif_type, "live_stream");
    assert_eq!(out[7].content, "Went live: Stream time");
    assert_eq!(out[7].created_at, 100);
}

#[test]
fn aggregate_notifications_event_id_falls_back_to_event_id() {
    let events = vec![
        nostr_event("e1", "pk1", 7, "+", &[]),
        nostr_event("e2", "pk2", 1, "hello", &[]),
    ];
    let out = aggregate_notifications(AggregateInput {
        events,
        existing_ids: vec![],
        live_stream_kind: 0,
    });
    assert_eq!(out[0].event_id, "e1");
    assert_eq!(out[1].event_id, "e2");
}

#[test]
fn aggregate_notifications_dedupes_existing_ids() {
    let events = vec![
        nostr_event("e1", "pk1", 7, "like+", &[&["e", "t1"]]),
        nostr_event("e2", "pk2", 7, "like+", &[&["e", "t2"]]),
    ];
    let dup = notif_id("reaction", "e2", "pk2");
    let out = aggregate_notifications(AggregateInput {
        events,
        existing_ids: vec![dup],
        live_stream_kind: 0,
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id, "reaction:e1:pk1");
}

#[test]
fn aggregate_notifications_dedupes_intra_batch_and_clamps_negative_timestamp() {
    let mut ev1 = nostr_event("e1", "pk1", 7, "like+", &[&["e", "t1"]]);
    ev1.created_at = -100.0;
    let mut ev2 = nostr_event("e1", "pk1", 7, "like+", &[&["e", "t1"]]);
    ev2.created_at = 1700000000.0;
    let out = aggregate_notifications(AggregateInput {
        events: vec![ev1, ev2],
        existing_ids: vec![],
        live_stream_kind: 0,
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].created_at, 0);
}

#[test]
fn aggregate_notifications_content_fallbacks() {
    let events = vec![
        nostr_event("e1", "pk1", 7, "", &[]),
        nostr_event("e2", "pk2", 7, "+", &[]),
        nostr_event("e3", "pk3", 7, "nice pic", &[]),
        nostr_event("e4", "pk4", 9735, "", &[]),
        nostr_event("e5", "pk5", 1, "", &[&["e", "t1"]]),
        nostr_event("e6", "pk6", 30311, "", &[]),
    ];
    let out = aggregate_notifications(AggregateInput {
        events,
        existing_ids: vec![],
        live_stream_kind: 30311,
    });
    assert_eq!(out[0].content, "Liked your post");
    assert_eq!(out[1].content, "Liked your post");
    assert_eq!(out[2].content, "nice pic");
    assert_eq!(out[3].content, "Sent you a zap");
    assert_eq!(out[4].content, "Replied to your post");
    assert_eq!(out[5].content, "Went live!");
}

#[test]
fn aggregate_notifications_empty_inputs() {
    assert_eq!(
        aggregate_notifications(AggregateInput {
            events: vec![],
            existing_ids: vec![],
            live_stream_kind: 0,
        })
        .len(),
        0
    );
    assert_eq!(aggregate_notifications_json("garbage"), "[]");
    assert_eq!(aggregate_notifications_json(""), "[]");
    assert_eq!(aggregate_notifications_json(r#"{"events":[]}"#), "[]");
}

#[test]
fn aggregate_notifications_json_roundtrip() {
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
    assert_eq!(v[0]["id"], "reaction:e1:pk1");
    assert_eq!(v[0]["content"], "Liked your post");
}

#[test]
fn notification_output_serializes_with_renamed_type() {
    let n = NotificationOutput {
        id: "like:e1:pk1".into(),
        notif_type: "reaction".into(),
        event_id: "t1".into(),
        from_pubkey: "pk1".into(),
        content: "Liked your post".into(),
        created_at: 100,
    };
    let v: serde_json::Value = serde_json::to_value(&n).unwrap();
    assert_eq!(v["type"], "reaction");
    assert_eq!(v["event_id"], "t1");
    assert_eq!(v["created_at"], 100);
}

#[test]
fn notif_id_normalizes_pubkey_casing() {
    assert_eq!(
        notif_id("reaction", "ev1", "ABCDEF123456"),
        "reaction:ev1:abcdef123456"
    );
    assert_eq!(
        notif_id("reaction", "ev1", "abcdef123456"),
        notif_id("reaction", "ev1", "ABCDEF123456")
    );
}

#[test]
fn notification_json_caps_safely() {
    let huge = "a".repeat(2 * 1024 * 1024);
    assert_eq!(format_content_json(&huge), "");
    assert_eq!(notif_id_json(&huge), "");

    let huge_agg = "b".repeat(17 * 1024 * 1024);
    assert_eq!(aggregate_notifications_json(&huge_agg), "[]");
}

#[test]
fn aggregate_notifications_friend_request_casing_and_limits() {
    let ev = nostr_event(
        "ev1",
        "PK_SENDER",
        1, // KIND_TEXT_NOTE
        "Let's be friends",
        &[&["t", "Friend-Request"]],
    );

    let res = aggregate_notifications(AggregateInput {
        events: vec![ev],
        existing_ids: vec![],
        live_stream_kind: 0,
    });

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].notif_type, "friend_request");
    assert_eq!(res[0].id, "friend_request:ev1:pk_sender");
}
