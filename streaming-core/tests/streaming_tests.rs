//! Integration tests for soshal-streaming-core.

use soshal_streaming_core::events::{
    chatrandom_available_content, chatrandom_peer_from_event, chatrandom_request_parts,
    live_chat_from_event, live_stream_from_event, story_content, story_from_event, stream_content,
};
use soshal_streaming_core::{filter_active_stories, merge_live_chat_messages};

fn event(tags: Vec<Vec<String>>) -> soshal_nostr_core::models::NostrEvent {
    soshal_nostr_core::models::NostrEvent {
        id: "e1".into(),
        pubkey: "pk1".into(),
        content: "content".into(),
        tags,
        created_at: 1000.0,
        kind: 1,
    }
}

fn msg(id: &str, created_at: i64) -> serde_json::Value {
    serde_json::json!({"id": id, "content": format!("m-{}", id), "created_at": created_at})
}

#[test]
fn story_content_builds_media_and_optional_text() {
    let content = story_content(
        &["https://cdn/x.png".into(), "https://cdn/y.png".into()],
        Some("hi"),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["media"].as_array().unwrap().len(), 2);
    assert_eq!(v["media"][0]["url"], "https://cdn/x.png");
    assert_eq!(v["media"][0]["type"], "image");
    assert_eq!(v["text"], "hi");

    let no_text = story_content(&["https://cdn/x.png".into()], None).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&no_text).unwrap();
    assert!(v2.get("text").is_none());
}

#[test]
fn story_from_event_maps_fields_and_default_audience() {
    let ev = event(vec![vec!["expiration".into(), "123".into()]]);
    let v = story_from_event(&ev);
    assert_eq!(v["id"], "e1");
    assert_eq!(v["pubkey"], "pk1");
    assert_eq!(v["created_at"], 1000u64);
    assert_eq!(v["expiration"], "123");
    assert_eq!(v["audience"], "public");
    assert_eq!(v["content"], "content");
}

#[test]
fn live_chat_from_event_maps_fields() {
    let v = live_chat_from_event(&event(vec![]));
    assert_eq!(v["id"], "e1");
    assert_eq!(v["created_at"], 1000u64);
    assert_eq!(v["content"], "content");
}

#[test]
fn stream_content_builds_json_and_skips_empty_category() {
    let c = stream_content("T", Some("s"), "https://stream", "live", Some("music")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&c).unwrap();
    assert_eq!(v["title"], "T");
    assert_eq!(v["summary"], "s");
    assert_eq!(v["stream_url"], "https://stream");
    assert_eq!(v["status"], "live");
    assert_eq!(v["category"], "music");

    let c2 = stream_content("T", None, "https://stream", "planned", Some("")).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&c2).unwrap();
    assert!(v2.get("summary").is_none());
    assert!(v2.get("category").is_none());
}

#[test]
fn live_stream_from_event_defaults_d_audience_category() {
    let ev = event(vec![
        vec!["d".into(), "my-stream".into()],
        vec!["status".into(), "live".into()],
    ]);
    let v = live_stream_from_event(&ev);
    assert_eq!(v["d_tag"], "my-stream");
    assert_eq!(v["status_tag"], "live");
    assert_eq!(v["audience"], "public");
    assert_eq!(v["category"], "Other");

    let v2 = live_stream_from_event(&event(vec![]));
    assert!(v2["d_tag"].is_null());
    assert!(v2["status_tag"].is_null());
}

#[test]
fn chatrandom_available_content_builds_json() {
    let c = chatrandom_available_content(&["anime".into(), "music".into()], "video", "video-only");
    let v: serde_json::Value = serde_json::from_str(&c).unwrap();
    assert_eq!(v["interests"][0], "anime");
    assert_eq!(v["media_type"], "video");
    assert_eq!(v["mode"], "video-only");
}

#[test]
fn chatrandom_peer_from_event_maps_fields() {
    let v = chatrandom_peer_from_event(&event(vec![]));
    assert_eq!(v["id"], "e1");
    assert_eq!(v["pubkey"], "pk1");
}

#[test]
fn chatrandom_request_parts_kind_and_content() {
    let (kind, content) = chatrandom_request_parts("request").unwrap();
    assert_eq!(kind, 20031);
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["type"], "request");

    let (kind2, _) = chatrandom_request_parts("accept").unwrap();
    assert_eq!(kind2, 20032);

    assert!(chatrandom_request_parts("reject").is_err());
}

#[test]
fn filter_active_stories_keeps_only_unexpired() {
    let stories = serde_json::json!([
        {"id": "s1", "created_at": 1000, "duration_secs": 100},
        {"id": "s2", "created_at": 950, "duration_secs": 100}, // expires at 1050
        {"id": "s3", "created_at": 500, "duration_secs": 0},
    ]);
    let out = filter_active_stories(stories.as_array().unwrap(), 1050);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["id"], "s1");

    let out2 = filter_active_stories(stories.as_array().unwrap(), 1000);
    assert_eq!(out2.len(), 2);
    assert!(out2.iter().all(|s| s["id"] != "s3"));
}

#[test]
fn merge_live_chat_messages_dedupes_and_sorts() {
    let existing = vec![msg("a", 10), msg("b", 30)];
    let incoming = vec![msg("b", 30), msg("c", 20), msg("d", 5)];
    let out = merge_live_chat_messages(existing, incoming);
    assert_eq!(out.len(), 4);
    let ids: Vec<&str> = out.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["d", "a", "c", "b"]);
}

#[test]
fn merge_live_chat_messages_drops_empty_ids_and_caps() {
    let incoming = vec![msg("", 0), msg("drop", 1)];
    let out = merge_live_chat_messages(vec![], incoming);
    assert_eq!(out.len(), 1);

    let existing: Vec<serde_json::Value> =
        (0..500i64).map(|i| msg(&format!("m{}", i), i)).collect();

    let out2 = merge_live_chat_messages(existing.clone(), vec![msg("new", 999)]);
    assert_eq!(out2.len(), 500);
    assert!(out2.iter().any(|m| m["id"] == "new"));
    assert!(out2[0]["id"] != "m0");
    assert_eq!(out2[0]["created_at"], existing[1]["created_at"]);
}

#[test]
fn merge_live_chat_messages_empty_id_in_existing_ignored() {
    let existing = vec![msg("", 5), msg("x", 10)];
    let incoming = vec![msg("y", 20)];
    let out = merge_live_chat_messages(existing, incoming);
    assert_eq!(out.len(), 3);
}

#[test]
fn stream_metadata_serde_and_derives() {
    use soshal_streaming_core::StreamMetadata;
    let meta = StreamMetadata {
        id: "s1".into(),
        title: "Test Stream".into(),
        summary: Some("Summary".into()),
        streaming_url: "https://stream.url/live".into(),
        status: "active".into(),
        starts_at: Some(100),
        ends_at: Some(200),
    };
    let json_str = serde_json::to_string(&meta).unwrap();
    let deserialized: StreamMetadata = serde_json::from_str(&json_str).unwrap();
    assert_eq!(meta, deserialized);
    assert_eq!(meta.clone(), deserialized);
}
