//! Integration tests for soshal-streaming-core.

use soshal_streaming_core::events::{
    chatrandom_available_content, chatrandom_peer_from_event, chatrandom_request_parts,
    live_chat_from_event, live_stream_from_event, story_content, story_from_event, stream_content,
};
use soshal_streaming_core::merge_live_chat_messages;

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
    let ev =
        soshal_test_util::nostr_event("content", vec![vec!["expiration".into(), "123".into()]]);
    let v = story_from_event(&ev);
    assert_eq!(v["id"], "id1");
    assert_eq!(v["pubkey"], "pk1");
    assert_eq!(v["created_at"], 100u64);
    assert_eq!(v["expiration"], "123");
    assert_eq!(v["audience"], "public");
    assert_eq!(v["content"], "content");
}

#[test]
fn live_chat_from_event_maps_fields() {
    let v = live_chat_from_event(&soshal_test_util::nostr_event("content", vec![]));
    assert_eq!(v["id"], "id1");
    assert_eq!(v["created_at"], 100u64);
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
fn story_content_rejects_empty_ssrf_and_excessive_urls() {
    assert!(story_content(&[], None).is_err());
    assert!(story_content(&[], Some("text only story")).is_ok());
    assert!(story_content(&["http://127.0.0.1/malicious.png".into()], None).is_err());
    assert!(story_content(&["javascript:alert(1)".into()], None).is_err());
    let excessive: Vec<String> = (0..33).map(|i| format!("https://cdn/img{i}.png")).collect();
    assert!(story_content(&excessive, None).is_err());
}

#[test]
fn stream_content_rejects_ssrf_and_bad_urls() {
    assert!(stream_content("T", None, "http://127.0.0.1:8080/stream", "live", None).is_err());
    assert!(stream_content("T", None, "http://169.254.169.254/latest", "live", None).is_err());
    assert!(stream_content("T", None, "javascript:evil()", "live", None).is_err());
    assert!(stream_content("T", None, "", "live", None).is_err());
}

#[test]
fn live_stream_from_event_defaults_d_audience_category() {
    let ev = soshal_test_util::nostr_event(
        "content",
        vec![
            vec!["d".into(), "my-stream".into()],
            vec!["status".into(), "live".into()],
        ],
    );
    let v = live_stream_from_event(&ev);
    assert_eq!(v["d_tag"], "my-stream");
    assert_eq!(v["status_tag"], "live");
    assert_eq!(v["audience"], "public");
    assert_eq!(v["category"], "Other");

    let v2 = live_stream_from_event(&soshal_test_util::nostr_event("content", vec![]));
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
    let v = chatrandom_peer_from_event(&soshal_test_util::nostr_event("content", vec![]));
    assert_eq!(v["id"], "id1");
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
fn test_events_clamp_negative_and_nan_created_at() {
    let mut ev = soshal_test_util::nostr_event("content", vec![]);
    ev.created_at = -100.0;
    let story = story_from_event(&ev);
    assert_eq!(story["created_at"], 0);

    ev.created_at = f64::NAN;
    let chat = live_chat_from_event(&ev);
    assert_eq!(chat["created_at"], 0);

    let stream = live_stream_from_event(&ev);
    assert_eq!(stream["created_at"], 0);

    let peer = chatrandom_peer_from_event(&ev);
    assert_eq!(peer["created_at"], 0);
}

#[test]
fn test_chatrandom_available_content_bounds() {
    let many_interests: Vec<String> = (0..100)
        .map(|i| format!("int_{}_{}", i, "x".repeat(100)))
        .collect();
    let res = chatrandom_available_content(&many_interests, &"v".repeat(50), &"m".repeat(50));
    let v: serde_json::Value = serde_json::from_str(&res).unwrap();
    let arr = v["interests"].as_array().unwrap();
    assert_eq!(arr.len(), 64);
    assert_eq!(arr[0].as_str().unwrap().len(), 64);
    assert_eq!(v["media_type"].as_str().unwrap().len(), 32);
    assert_eq!(v["mode"].as_str().unwrap().len(), 32);
}
