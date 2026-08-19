//! Integration tests for soshal-social-core: chatrandom scoring, interest
//! compatibility kernels, interest-based discovery and relation mapping.

use soshal_social_core::chatrandom::{
    compute_jaccard_score, match_group_chatrandom, rank_chatrandom_peers,
};
use soshal_social_core::compatibility::{
    basic_compatibility, interest_overlap, interest_overlap_details, jaccard_similarity,
};
use soshal_social_core::interest::discover_by_interest_json;
use soshal_social_core::relations::relation_entry_from_event;

// ─── chatrandom ─────────────────────────────────────────────────────────

#[test]
fn compute_jaccard_score_identical_and_disjoint() {
    assert_eq!(compute_jaccard_score(&["a".into()], &["a".into()]), 1.0);
    assert_eq!(
        compute_jaccard_score(&["a".into(), "b".into()], &["b".into(), "c".into()]),
        1.0 / 3.0
    );
    assert_eq!(compute_jaccard_score(&["a".into()], &["b".into()]), 0.0);
}

#[test]
fn compute_jaccard_score_empty_is_zero() {
    assert_eq!(compute_jaccard_score(&[], &["a".into()]), 0.0);
    assert_eq!(compute_jaccard_score(&["a".into()], &[]), 0.0);
    assert_eq!(compute_jaccard_score(&[], &[]), 0.0);
}

#[test]
fn compute_jaccard_score_normalizes_case_and_whitespace() {
    assert_eq!(
        compute_jaccard_score(&[" Music ".into()], &["music".into()]),
        1.0
    );
    assert_eq!(
        compute_jaccard_score(&["Music".into()], &["music".into()]),
        1.0
    );
}

#[test]
fn rank_chatrandom_peers_scores_and_sorts_descending() {
    let input = serde_json::json!({
        "candidates": [
            {"pubkey": "bob", "interests": ["sports"], "expires_at": 0},
            {"pubkey": "alice", "interests": ["Music", "gaming"]},
            {"pubkey": "carol", "interests": []}
        ],
        "my_interests": ["music", "gaming"]
    });
    let out = rank_chatrandom_peers(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored.len(), 3);
    assert_eq!(scored[0]["pubkey"], "alice");
    assert_eq!(scored[0]["score"], 1.0);
    assert_eq!(scored[1]["pubkey"], "bob");
    assert_eq!(scored[1]["score"], 0.0);
    assert_eq!(scored[2]["pubkey"], "carol");
    assert_eq!(scored[2]["score"], 0.0);
}

#[test]
fn rank_chatrandom_peers_empty_my_interests_scores_zero() {
    let input = serde_json::json!({
        "candidates": [{"pubkey": "a", "interests": ["music"]}],
        "my_interests": []
    });
    let out = rank_chatrandom_peers(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored[0]["score"], 0.0);
}

#[test]
fn rank_chatrandom_peers_invalid_json_returns_empty_array() {
    assert_eq!(rank_chatrandom_peers("not json"), "[]");
    assert_eq!(rank_chatrandom_peers(""), "[]");
    assert_eq!(rank_chatrandom_peers(r#"{"candidates":[]}"#), "[]");
}

#[test]
fn match_group_chatrandom_scores_filters_full_rooms() {
    let input = serde_json::json!({
        "user_interests": ["music", "gaming"],
        "rooms": [
            {"room_id": "r1", "interests": ["music"], "participant_count": 1, "max_participants": 4},
            {"room_id": "r2", "interests": ["sports"], "participant_count": 0, "max_participants": 4},
            {"room_id": "r3", "interests": ["music"], "participant_count": 4, "max_participants": 4},
            {"room_id": "r4", "interests": ["music"], "participant_count": 0, "max_participants": 0}
        ]
    });
    let out = match_group_chatrandom(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored.len(), 2);
    assert_eq!(scored[0]["room_id"], "r1");
    assert!((scored[0]["score"].as_f64().unwrap() - (0.5 * 0.7 + 0.25 * 0.3)).abs() < 1e-9);
    assert_eq!(scored[1]["room_id"], "r2");
    assert_eq!(scored[1]["score"], 0.0);
}

#[test]
fn match_group_chatrandom_no_overlap_ranks_by_fullness_only() {
    let input = serde_json::json!({
        "user_interests": ["music"],
        "rooms": [
            {"room_id": "r1", "interests": ["sports"], "participant_count": 3, "max_participants": 4},
            {"room_id": "r2", "interests": ["sports"], "participant_count": 0, "max_participants": 4}
        ]
    });
    let out = match_group_chatrandom(&input.to_string());
    let scored: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(scored[0]["room_id"], "r1");
    assert!((scored[0]["score"].as_f64().unwrap() - 3.0 / 4.0 * 0.3).abs() < 1e-9);
}

#[test]
fn match_group_chatrandom_invalid_json_returns_empty_array() {
    assert_eq!(match_group_chatrandom("garbage"), "[]");
    assert_eq!(match_group_chatrandom(r#"{"rooms":[]}"#), "[]");
}

// ─── compatibility ──────────────────────────────────────────────────────

#[test]
fn interest_overlap_details_counts_and_dedupes() {
    let a = vec![
        "Music".to_string(),
        "music".to_string(),
        " Travel ".to_string(),
    ];
    let b = vec!["music".to_string(), "Gaming".to_string()];
    let (common, union, len_a, len_b) = interest_overlap_details(&a, &b, false);
    assert_eq!(common, vec!["music"]);
    assert_eq!(union, 4);
    assert_eq!(len_a, 3);
    assert_eq!(len_b, 2);

    let (common, union, len_a, len_b) = interest_overlap_details(&a, &b, true);
    assert_eq!(common, vec!["Music"], "common is deduplicated by match key");
    assert_eq!(union, 3);
    assert_eq!(len_a, 2);
    assert_eq!(len_b, 2);
}

#[test]
fn interest_overlap_details_skips_empty_and_whitespace_items() {
    let a = vec!["".to_string(), "  ".to_string(), "x".to_string()];
    let b = vec!["x".to_string()];
    let (common, union, len_a, len_b) = interest_overlap_details(&a, &b, false);
    assert_eq!(common, vec!["x"]);
    assert_eq!(union, 1);
    assert_eq!(len_a, 1);
    assert_eq!(len_b, 1);
}

#[test]
fn interest_overlap_matches_case_sensitively_by_default() {
    let a = vec!["Music".to_string(), "music".to_string()];
    let b = vec!["music".to_string()];
    let (common, union) = interest_overlap(&a, &b, false);
    assert_eq!(common, vec!["music"]);
    assert_eq!(union, 2);
    let (common, _) = interest_overlap(&a, &b, true);
    assert_eq!(
        common,
        vec!["Music"],
        "common is deduplicated by case-insensitive key"
    );
}

#[test]
fn basic_compatibility_is_common_over_max() {
    assert_eq!(
        basic_compatibility(&["a".into(), "b".into()], &["b".into(), "c".into()]),
        0.5
    );
    assert_eq!(basic_compatibility(&["x".into()], &["x".into()]), 1.0);
    assert_eq!(
        basic_compatibility(&["a".into()], &["b".into(), "c".into()]),
        0.0
    );
}

#[test]
fn basic_compatibility_empty_defaults_midpoint() {
    assert_eq!(basic_compatibility(&[], &["a".into()]), 0.5);
    assert_eq!(basic_compatibility(&["a".into()], &[]), 0.5);
    assert_eq!(basic_compatibility(&[], &[]), 0.5);
}

#[test]
fn basic_compatibility_dedupes_duplicate_items() {
    // common and max both use deduplicated key sets — duplicates don't
    // inflate the score.
    assert_eq!(
        basic_compatibility(&["a".into(), "a".into()], &["a".into()]),
        1.0
    );
    assert_eq!(
        basic_compatibility(&["a".into(), "a".into()], &["b".into()]),
        0.0
    );
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
    assert_eq!(jaccard_similarity(&["a".into()], &[], true), 0.5);
}

// ─── interest ───────────────────────────────────────────────────────────

#[test]
fn discover_by_interest_json_matches_and_excludes_self_and_contacts() {
    let input = serde_json::json!({
        "events": [
            {"pubkey": "p1", "content": "I love Rust programming"},
            {"pubkey": "p2", "content": "I play games"},
            {"pubkey": "me", "content": "I love rust too"},
            {"pubkey": "contact", "content": "rust everywhere"}
        ],
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": ["contact"],
        "limit": 10
    });
    let out = discover_by_interest_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["pubkey"], "p1");
    assert_eq!(v[0]["distance"], 2);
    assert!(v[0]["reason"].as_str().unwrap().contains("rust"));
}

#[test]
fn discover_by_interest_json_case_insensitive_and_multi_tag() {
    let input = serde_json::json!({
        "events": [
            {"pubkey": "p1", "content": "Go RUST go"},
            {"pubkey": "p2", "content": "cooking show"}
        ],
        "tags": ["RUST", "cooking"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 10
    });
    let out = discover_by_interest_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert!(v[0]["reason"].as_str().unwrap().contains("rust"));
}

#[test]
fn discover_by_interest_json_respects_limit() {
    let events: Vec<serde_json::Value> = (0..5)
        .map(|i| serde_json::json!({"pubkey": format!("p{i}"), "content": "rust here"}))
        .collect();
    let input = serde_json::json!({
        "events": events,
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 2
    });
    let out = discover_by_interest_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn discover_by_interest_json_no_match_and_invalid_input() {
    let input = serde_json::json!({
        "events": [{"pubkey": "p1", "content": "no tags here"}],
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 10
    });
    assert_eq!(discover_by_interest_json(&input.to_string()), "[]");
    assert_eq!(discover_by_interest_json("garbage"), "[]");
    assert_eq!(discover_by_interest_json(""), "[]");
}

#[test]
fn discover_by_interest_json_rejects_oversized_inputs() {
    let base = serde_json::json!({
        "events": [],
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 10
    });
    let too_many_events = serde_json::json!({
        "events": (0..10_001).map(|i| serde_json::json!({"pubkey": format!("p{i}"), "content": "x"})).collect::<Vec<_>>(),
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 10
    });
    let too_many_tags = serde_json::json!({
        "events": [{"pubkey": "p1", "content": "x"}],
        "tags": (0..101).map(|i| format!("t{i}")).collect::<Vec<_>>(),
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 10
    });
    let too_big_limit = serde_json::json!({
        "events": [],
        "tags": ["rust"],
        "self_pubkey": "me",
        "self_contacts": [],
        "limit": 100_001
    });
    assert_eq!(
        discover_by_interest_json(&too_many_events.to_string()),
        "[]"
    );
    assert_eq!(discover_by_interest_json(&too_many_tags.to_string()), "[]");
    assert_eq!(discover_by_interest_json(&too_big_limit.to_string()), "[]");
    let _ = base;
}

// ─── relations ──────────────────────────────────────────────────────────

#[test]
fn relation_entry_from_event_maps_fields() {
    let v = relation_entry_from_event(&soshal_test_util::nostr_event_kind(
        31989,
        "good people",
        vec![vec!["p".to_string(), "pk9".to_string()]],
    ));
    assert_eq!(v["id"], "id1");
    assert_eq!(v["pubkey"], "pk1");
    assert_eq!(v["content"], "good people");
    assert_eq!(v["created_at"], 100u64);
}
