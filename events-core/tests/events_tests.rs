use soshal_events_core::checkin::can_checkin;
use soshal_events_core::deletion::{
    extract_deletion_ids, MAX_OUTPUT_IDS, MAX_TAGS, MAX_TAG_FIELDS, MAX_TAG_FIELD_LEN,
};
use soshal_events_core::event::expiry::get_expiry_from_tags;
use soshal_events_core::event::interest::compute_interest_score;

#[test]
fn test_checkin_window() {
    assert!(can_checkin(1000, 2000, 1500, 300));
    assert!(!can_checkin(1000, 2000, 500, 300));
    assert!(!can_checkin(1000, 2000, 2500, 300));
}

#[test]
fn test_buffer_early() {
    assert!(can_checkin(1000, 2000, 800, 300));
    assert!(!can_checkin(1000, 2000, 600, 300));
}

#[test]
fn test_exact_boundaries_and_zero_buffer() {
    assert!(can_checkin(1000, 2000, 1000, 0));
    assert!(!can_checkin(1000, 2000, 2000, 0));
    assert!(!can_checkin(1000, 2000, 999, 0));
    assert!(!can_checkin(1000, 2000, 2001, 0));
}

#[test]
fn extracts_e_tag_ids() {
    let tags = vec![
        vec!["e".to_string(), "abc".to_string()],
        vec!["p".to_string(), "pubkey".to_string()],
        vec!["e".to_string(), "def".to_string()],
    ];
    assert_eq!(extract_deletion_ids(&tags), vec!["abc", "def"]);
}

#[test]
fn empty_tags_returns_empty() {
    assert!(extract_deletion_ids(&[]).is_empty());
}

#[test]
fn skips_empty_tags() {
    let tags: Vec<Vec<String>> = vec![vec![]];
    assert!(extract_deletion_ids(&tags).is_empty());
}

#[test]
fn skips_oversized_field() {
    let big = "x".repeat(MAX_TAG_FIELD_LEN + 1);
    let tags = vec![vec!["e".to_string(), big]];
    assert!(extract_deletion_ids(&tags).is_empty());
}

#[test]
fn deduplicates_ids() {
    let tags = vec![
        vec!["e".to_string(), "abc".to_string()],
        vec!["e".to_string(), "abc".to_string()],
        vec!["e".to_string(), "def".to_string()],
    ];
    assert_eq!(extract_deletion_ids(&tags), vec!["abc", "def"]);
}

#[test]
fn caps_output_id_count() {
    let tags: Vec<Vec<String>> = (0..(MAX_OUTPUT_IDS as u32 + 100))
        .map(|i| vec!["e".to_string(), format!("id{}", i)])
        .collect();
    let out = extract_deletion_ids(&tags);
    assert_eq!(out.len(), MAX_TAGS);
}

#[test]
fn ignores_oversized_tag_field_count() {
    let mut tag = vec!["e".to_string(), "abc".to_string()];
    for i in 0..(MAX_TAG_FIELDS + 5) {
        tag.push(format!("f{}", i));
    }
    let tags = vec![tag];
    assert!(extract_deletion_ids(&tags).is_empty());
}

#[test]
fn handles_many_tags_without_panic() {
    let tags: Vec<Vec<String>> = (0..MAX_TAGS)
        .map(|i| vec!["e".to_string(), format!("id{}", i)])
        .collect();
    let out = extract_deletion_ids(&tags);
    assert_eq!(out.len(), MAX_TAGS);
}

#[test]
fn get_expiry_from_tags_found() {
    let tags = vec![
        vec!["e".to_string(), "abc".to_string()],
        vec!["expires_at".to_string(), "1700000000".to_string()],
    ];
    assert_eq!(get_expiry_from_tags(&tags), 1700000000);
}

#[test]
fn get_expiry_from_tags_not_found() {
    let tags = vec![vec!["e".to_string(), "abc".to_string()]];
    assert_eq!(get_expiry_from_tags(&tags), 0);
}

#[test]
fn compute_interest_score_some_common() {
    let my = vec!["music".to_string(), "art".to_string(), "tech".to_string()];
    let peer = vec![
        "music".to_string(),
        "sports".to_string(),
        "tech".to_string(),
    ];
    let result = compute_interest_score(&my, &peer);
    assert!((result.score - 0.5).abs() < 0.001);
    assert_eq!(result.common.len(), 2);
}

#[test]
fn compute_interest_score_no_common() {
    let my = vec!["a".to_string(), "b".to_string()];
    let peer = vec!["c".to_string(), "d".to_string()];
    let result = compute_interest_score(&my, &peer);
    assert_eq!(result.score, 0.0);
    assert!(result.common.is_empty());
}

#[test]
fn test_extract_deletion_ids_json() {
    use soshal_events_core::deletion::extract_deletion_ids_json;
    let input = serde_json::json!({
        "tags": [
            ["e", "deleted_event_1"],
            ["e", "deleted_event_2"]
        ]
    });
    let out = extract_deletion_ids_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[0], "deleted_event_1");
    assert_eq!(extract_deletion_ids_json("garbage"), "[]");
}
