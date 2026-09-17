//! Integration tests for groups-core group event parsing (posts, chat
//! messages, channels): hostile-input guards and tag extraction.

use serde_json::json;
use soshal_groups_core::group::channels::parse_group_channels_json;
use soshal_groups_core::group::chat::parse_group_chat_messages_json;
use soshal_groups_core::group::posts::parse_group_posts_json;

fn post_event(
    id: &str,
    content: &str,
    tags: Vec<Vec<String>>,
    created_at: f64,
) -> serde_json::Value {
    json!({
        "id": id,
        "pubkey": "pk",
        "content": content,
        "tags": tags,
        "created_at": created_at,
        "kind": 42
    })
}

fn tag(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> Vec<T> {
    serde_json::from_str(json).unwrap()
}

#[test]
fn posts_parse_valid_events() {
    let input = json!({
        "events": [
            post_event("e1", "hello", vec![tag(&["d", "grp1"]), tag(&["h", "chan1"]), tag(&["image", "a.jpg"])], 1000.0),
            post_event("e2", "other group", vec![tag(&["d", "grp2"])], 2000.0)
        ],
        "groupId": "grp1"
    });
    let out = parse::<serde_json::Value>(&parse_group_posts_json(&input.to_string()));
    assert_eq!(out.len(), 1, "non-matching d tag must be dropped");
    assert_eq!(out[0]["id"], "e1");
    assert_eq!(out[0]["channelId"], "chan1");
    assert_eq!(out[0]["images"][0], "a.jpg");
    assert_eq!(out[0]["createdAt"], 1000.0 * 1000.0);
    assert_eq!(out[0]["groupId"], "grp1");
}

#[test]
fn posts_extract_imeta_videos() {
    let input = json!({
        "events": [
            post_event("e1", "v", vec![
                tag(&["d", "g"]),
                tag(&["imeta", "url=https://x/v.mp4", "m=video/mp4"]),
                tag(&["imeta", "url=https://x/a.gif", "m=image/gif"]),
                tag(&["imeta", "url=https://x/other", "m=image/png"]),
            ], 1.0)
        ],
        "group_id": "g"
    });
    let out = parse::<serde_json::Value>(&parse_group_posts_json(&input.to_string()));
    let videos = out[0]["videos"].as_array().unwrap();
    assert_eq!(videos.len(), 2, "video + gif mime qualify");
}

#[test]
fn posts_reject_oversize_and_malformed() {
    let big_tags: Vec<Vec<String>> = (0..100_001).map(|_| tag(&["d", "g"])).collect();
    let input = json!({ "events": [post_event("e1", "", big_tags, 1.0)], "group_id": "g" });
    assert_eq!(parse_group_posts_json(&input.to_string()), "[]");
    assert_eq!(parse_group_posts_json("not json"), "[]");
    let long_group = json!({ "events": [], "group_id": "x".repeat(600) });
    assert_eq!(parse_group_posts_json(&long_group.to_string()), "[]");
    let huge_tags = json!({
        "events": [post_event("e1", "", vec![vec!["d".to_string(); 100_001]], 1.0)],
        "group_id": "g"
    });
    assert_eq!(parse_group_posts_json(&huge_tags.to_string()), "[]");
}

#[test]
fn posts_oversized_image_url_dropped() {
    let input = json!({
        "events": [post_event("e1", "", vec![tag(&["d", "g"]), tag(&["image", &"x".repeat(5000)])], 1.0)],
        "group_id": "g"
    });
    let out = parse::<serde_json::Value>(&parse_group_posts_json(&input.to_string()));
    assert!(out[0]["images"].as_array().unwrap().is_empty());
}

#[test]
fn chat_parses_and_filters_channel() {
    let input = json!({
        "events": [
            post_event("m1", "hi", vec![tag(&["d", "g"]), tag(&["h", "c1"]), tag(&["image", "pic.jpg"])], 5.0),
            post_event("m2", "other chan", vec![tag(&["d", "g"]), tag(&["h", "c2"])], 6.0)
        ],
        "group_id": "g",
        "channel_id": "c1"
    });
    let out = parse::<serde_json::Value>(&parse_group_chat_messages_json(&input.to_string()));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["id"], "m1");
    assert_eq!(out[0]["image_url"], "pic.jpg");
    assert_eq!(out[0]["created_at"], 5.0 * 1000.0);
}

#[test]
fn chat_handles_video_tag_and_guards() {
    let input = json!({
        "events": [post_event("m1", "v", vec![tag(&["d", "g"]), tag(&["video", "clip.mp4"])], 1.0)],
        "group_id": "g"
    });
    let out = parse::<serde_json::Value>(&parse_group_chat_messages_json(&input.to_string()));
    assert_eq!(out[0]["video_url"], "clip.mp4");
    assert_eq!(parse_group_chat_messages_json("garbage"), "[]");
    let long_channel = json!({
        "events": [], "group_id": "g", "channel_id": "c".repeat(600)
    });
    assert_eq!(
        parse_group_chat_messages_json(&long_channel.to_string()),
        "[]"
    );
}

#[test]
fn channels_parse_and_guard() {
    let input = json!({
        "events": [{
            "id": "ch1",
            "pubkey": "pk",
            "content": "",
            "tags": [
                ["d", "ch1"], ["g", "grp1"], ["name", "General"],
                ["description", "all talk"], ["category", "social"],
                ["type", "text"], ["position", "3"], ["slow_mode_seconds", "120"]
            ],
            "created_at": 42.0,
            "kind": 40
        }]
    });
    let out = parse::<serde_json::Value>(&parse_group_channels_json(&input.to_string()));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["id"], "ch1");
    assert_eq!(out[0]["group_id"], "grp1");
    assert_eq!(out[0]["name"], "General");
    assert_eq!(out[0]["description"], "all talk");
    assert_eq!(out[0]["category"], "social");
    assert_eq!(out[0]["channel_type"], "text");
    assert_eq!(out[0]["position"], 3);
    assert_eq!(out[0]["slow_mode_seconds"], 120);
    assert_eq!(out[0]["created_by"], "pk");
    assert_eq!(parse_group_channels_json("[]"), "[]");
    assert_eq!(parse_group_channels_json("nope"), "[]");
}

#[test]
fn channels_reject_bad_position_and_slow_mode() {
    let input = json!({
        "events": [{
            "id": "ch1",
            "pubkey": "pk",
            "content": "",
            "tags": [
                ["d", "ch1"], ["g", "g"], ["name", "N"],
                ["position", "9999999999999"],
                ["slow_mode_seconds", "-5"]
            ],
            "created_at": 1.0,
            "kind": 40
        }]
    });
    let out = parse::<serde_json::Value>(&parse_group_channels_json(&input.to_string()));
    assert!(out[0]["position"].is_null());
    assert!(out[0]["slow_mode_seconds"].is_null());
}

#[test]
fn posts_reject_ssrf_and_script_media() {
    let input = json!({
        "events": [{
            "id": "e_ssrf",
            "pubkey": "pk",
            "content": "",
            "tags": [
                ["d", "g"],
                ["image", "http://127.0.0.1/private.jpg"],
                ["image", "javascript:alert(1)"],
                ["image", "data:text/html;base64,PHNjcmlwdD4="],
                ["image", "https://example.com/safe.jpg"],
                ["imeta", "url=http://169.254.169.254/secret.mp4", "m=video/mp4"],
                ["imeta", "url=https://example.com/safe.mp4", "m=video/mp4"]
            ],
            "created_at": 1.0,
            "kind": 1
        }],
        "group_id": "g"
    });
    let out = parse::<serde_json::Value>(&parse_group_posts_json(&input.to_string()));
    let images = out[0]["images"].as_array().unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0], "https://example.com/safe.jpg");

    let videos = out[0]["videos"].as_array().unwrap();
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0], "https://example.com/safe.mp4");
}
