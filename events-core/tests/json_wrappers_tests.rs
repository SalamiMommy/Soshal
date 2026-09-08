use soshal_events_core::event::expiry::{get_expiry_from_tags, get_expiry_from_tags_json};
use soshal_events_core::event::interest::{compute_interest_score, compute_interest_score_json};

#[test]
fn get_expiry_from_tags_json_valid_and_bad_input() {
    assert_eq!(
        get_expiry_from_tags_json(r#"{"tags":[["expires_at","1710000000"],["t","x"]]}"#),
        1710000000
    );
    assert_eq!(
        get_expiry_from_tags_json(r#"{"tags":[["expires_at","not-a-number"]]}"#),
        0
    );
    assert_eq!(get_expiry_from_tags_json(r#"{"tags":[]}"#), 0);
    assert_eq!(get_expiry_from_tags_json("not-json"), 0);
    assert_eq!(get_expiry_from_tags_json(""), 0);
    assert_eq!(get_expiry_from_tags_json(r#"{"tags":[["other","1"]]}"#), 0);
}

#[test]
fn get_expiry_from_tags_json_matches_direct() {
    let tags = vec![vec!["expires_at".into(), "1720000000".into()]];
    assert_eq!(get_expiry_from_tags(&tags), 1720000000);
    assert_eq!(
        get_expiry_from_tags_json(r#"{"tags":[["expires_at","1720000000"]]}"#),
        get_expiry_from_tags(&tags)
    );
}

#[test]
fn compute_interest_score_json_roundtrip() {
    let my = ["nostr".to_string(), "rust".to_string()];
    let peer = ["rust".to_string(), "ai".to_string()];
    let direct = compute_interest_score(&my, &peer);
    let via_json = compute_interest_score_json(
        r#"{"myInterests":["nostr","rust"],"peerInterests":["rust","ai"]}"#,
    );
    let v: serde_json::Value = serde_json::from_str(&via_json).unwrap();
    assert_eq!(v["score"].as_f64().unwrap(), direct.score, "got {via_json}");
    assert_eq!(v["common"][0].as_str().unwrap(), "rust");
    assert_eq!(v["common"].as_array().unwrap().len(), direct.common.len());
}

#[test]
fn compute_interest_score_json_bad_input_fallback() {
    let v: serde_json::Value =
        serde_json::from_str(&compute_interest_score_json("not-json")).unwrap();
    assert_eq!(v["score"], 0.0);
    assert_eq!(v["common"].as_array().unwrap().len(), 0);
    let v: serde_json::Value =
        serde_json::from_str(&compute_interest_score_json(r#"{"bad":1}"#)).unwrap();
    assert_eq!(v["score"], 0.0);
}
