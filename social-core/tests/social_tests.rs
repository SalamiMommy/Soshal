//! Integration tests for soshal-social-core.

use soshal_social_core::chatrandom::{match_group_chatrandom, rank_chatrandom_peers};
use soshal_social_core::compatibility::{
    basic_compatibility, interest_overlap, jaccard_similarity,
};
use soshal_social_core::relations::relation_entry_from_event;

fn event(tags: Vec<Vec<String>>) -> soshal_nostr_core::models::NostrEvent {
    soshal_nostr_core::models::NostrEvent {
        id: "e1".into(),
        pubkey: "pk1".into(),
        content: "content".into(),
        tags,
        created_at: 1000.0,
        kind: 31989,
    }
}

#[test]
fn interest_overlap_matches_case_sensitively_by_default() {
    let a = vec![
        "Music".to_string(),
        "music".to_string(),
        " Travel ".to_string(),
    ];
    let b = vec!["music".to_string(), "Gaming".to_string()];
    let (common, union) = interest_overlap(&a, &b, false);
    assert_eq!(common, vec!["music"]);
    assert_eq!(union, 4);
}

#[test]
fn interest_overlap_case_insensitive_returns_original_case() {
    let a = vec![
        "Music".to_string(),
        "music".to_string(),
        "Travel".to_string(),
    ];
    let b = vec!["music".to_string(), "Gaming".to_string()];
    let (common, union) = interest_overlap(&a, &b, true);
    assert_eq!(common, vec!["Music", "music"]);
    assert_eq!(union, 3);
}

#[test]
fn interest_overlap_skips_empty_items() {
    let a = vec!["".to_string(), "  ".to_string(), "x".to_string()];
    let b = vec!["x".to_string()];
    let (common, union) = interest_overlap(&a, &b, false);
    assert_eq!(common, vec!["x"]);
    assert_eq!(union, 1);
}

#[test]
fn basic_compatibility_is_common_over_max() {
    assert_eq!(
        basic_compatibility(&["a".into(), "b".into()], &["b".into(), "c".into()]),
        0.5
    );
    assert_eq!(basic_compatibility(&["x".into()], &["x".into()]), 1.0);
    assert_eq!(
        basic_compatibility(&["a".into()], &["b".into(), "c".into(), "d".into()]),
        0.0
    );
}

#[test]
fn basic_compatibility_empty_defaults_midpoint() {
    assert_eq!(basic_compatibility(&[], &["a".into()]), 0.5);
    assert_eq!(basic_compatibility(&["a".into()], &[]), 0.5);
}

#[test]
fn jaccard_similarity_scores_common_over_union() {
    let a = vec!["a".into(), "b".into()];
    let b = vec!["b".into(), "c".into()];
    assert_eq!(jaccard_similarity(&a, &b, false), 1.0 / 3.0);
}

#[test]
fn jaccard_similarity_case_flags() {
    assert_eq!(
        jaccard_similarity(&["Music".into()], &["music".into()], false),
        0.0
    );
    assert_eq!(
        jaccard_similarity(&["Music".into()], &["music".into()], true),
        1.0
    );
}

#[test]
fn jaccard_similarity_empty_defaults_midpoint() {
    assert_eq!(jaccard_similarity(&[], &["a".into()], false), 0.5);
}

#[test]
fn relation_entry_from_event_maps_fields() {
    let v = relation_entry_from_event(&event(vec![]));
    assert_eq!(v["id"], "e1");
    assert_eq!(v["pubkey"], "pk1");
    assert_eq!(v["content"], "content");
    assert_eq!(v["created_at"], 1000u64);
}

#[test]
fn rank_chatrandom_peers_orders_by_score() {
    let input = serde_json::json!({
        "candidates": [
            {"pubkey": "bob", "interests": ["sports"], "expires_at": 0},
            {"pubkey": "alice", "interests": ["music"], "expires_at": 100},
            {"pubkey": "carol", "interests": []}
        ],
        "my_interests": ["music", "gaming"]
    });
    let out = rank_chatrandom_peers(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored.len(), 3);
    assert_eq!(scored[0]["pubkey"], "alice");
    assert_eq!(scored[0]["score"], 0.5);
    assert_eq!(scored[1]["pubkey"], "bob");
    assert_eq!(scored[1]["score"], 0.0);
    assert_eq!(scored[2]["pubkey"], "carol");
}

#[test]
fn rank_chatrandom_peers_invalid_input_returns_empty_array() {
    assert_eq!(rank_chatrandom_peers("not json"), "[]");
    assert_eq!(rank_chatrandom_peers(""), "[]");
}

#[test]
fn match_group_chatrandom_scores_and_filters_full_rooms() {
    let input = serde_json::json!({
        "user_interests": ["music", "gaming"],
        "rooms": [
            {"room_id": "r1", "interests": ["music"], "participant_count": 1, "max_participants": 4},
            {"room_id": "r2", "interests": ["sports"], "participant_count": 0, "max_participants": 4},
            {"room_id": "r3", "interests": ["music"], "participant_count": 4, "max_participants": 4}
        ]
    });
    let out = match_group_chatrandom(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored.len(), 2);
    assert_eq!(scored[0]["room_id"], "r1");
    assert_eq!(scored[0]["score"], 0.5 * 0.7 + 0.25 * 0.3);
    assert_eq!(scored[1]["room_id"], "r2");
    assert_eq!(scored[1]["score"], 0.0);
}

#[test]
fn match_group_chatrandom_empty_user_interests_still_ranks_by_fullness() {
    let input = serde_json::json!({
        "user_interests": [],
        "rooms": [
            {"room_id": "r1", "interests": ["music"], "participant_count": 3, "max_participants": 4},
            {"room_id": "r2", "interests": ["sports"], "participant_count": 0, "max_participants": 4}
        ]
    });
    let out = match_group_chatrandom(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored.len(), 2);
    assert_eq!(scored[0]["room_id"], "r1");
    let score: f64 = scored[0]["score"].as_f64().unwrap();
    assert!((score - (3.0 / 4.0 * 0.3)).abs() < 1e-9);
}

#[test]
fn match_group_chatrandom_invalid_input_returns_empty_array() {
    assert_eq!(match_group_chatrandom("garbage"), "[]");
}

#[test]
fn test_discover_by_interest_json() {
    use soshal_social_core::interest::discover_by_interest_json;
    let input = serde_json::json!({
        "events": [
            {"pubkey": "p1", "content": "I love rust programming"},
            {"pubkey": "p2", "content": "I play games"},
            {"pubkey": "me", "content": "I love rust too"}
        ],
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 10
    });
    let out = discover_by_interest_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["pubkey"], "p1");
    assert!(v[0]["reason"].as_str().unwrap().contains("rust"));

    assert_eq!(discover_by_interest_json("garbage"), "[]");
}
