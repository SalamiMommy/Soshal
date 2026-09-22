//! Rate limiting tests

use soshal_network_core::rate_limit::rate_limit_check_json;

#[test]
fn rate_limit_hostile_attempts_and_now_do_not_overflow() {
    // L10: u32::MAX attempts / now near u64::MAX must saturate, never
    // wrap into a reset that un-blocks the limiter.
    let input = r#"{
        "attempts": 4294967295,
        "first_attempt": 1,
        "now": 18446744073709551615,
        "max_attempts": 4294967295,
        "window_ms": 18446744073709551615,
        "blocked_until": null
    }"#;
    let result = rate_limit_check_json(input);
    assert!(
        result.contains("\"allowed\":false"),
        "max attempts + unsaturated overflow must stay blocked"
    );
    assert!(result.contains("\"new_blocked_until\":18446744073709551615"));
    // An attempt increment past max-1 must not panic or wrap to 0.
    let input = r#"{
        "attempts": 4294967294,
        "first_attempt": 1,
        "now": 2,
        "max_attempts": 4294967295,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"new_attempts\":4294967295"));
}

#[test]
fn rate_limit_allows_first_attempt() {
    let input = r#"{
        "attempts": 0,
        "first_attempt": 1000,
        "now": 2000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"allowed\":true"));
}

#[test]
fn rate_limit_blocks_when_max_reached() {
    let input = r#"{
        "attempts": 5,
        "first_attempt": 1000,
        "now": 2000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"allowed\":false"));
    assert!(result.contains("\"retry_after\""));
}

#[test]
fn rate_limit_resets_after_window() {
    let input = r#"{
        "attempts": 5,
        "first_attempt": 1000,
        "now": 70000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"allowed\":true"));
    assert!(result.contains("\"new_attempts\":1"));
}

#[test]
fn rate_limit_respects_existing_block() {
    let input = r#"{
        "attempts": 5,
        "first_attempt": 1000,
        "now": 2000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": 10000
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"allowed\":false"));
    assert!(result.contains("\"retry_after\":8000"));
}

#[test]
fn rate_limit_block_expires_and_window_resets() {
    // When both block expires AND window expires, it should allow
    let input = r#"{
        "attempts": 5,
        "first_attempt": 1000,
        "now": 70000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": 10000
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"allowed\":true"));
    assert!(result.contains("\"new_attempts\":1"));
}

#[test]
fn rate_limit_expires_block() {
    // When block expires but window hasn't expired and attempts are still at max,
    // it will block again. The block expiring just means the specific block time
    // has passed, but the rate limit state (attempts) is still at max.
    let input = r#"{
        "attempts": 5,
        "first_attempt": 1000,
        "now": 15000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": 10000
    }"#;
    let result = rate_limit_check_json(input);
    // Still blocked because attempts are at max and window hasn't expired
    assert!(result.contains("\"allowed\":false"));
}

#[test]
fn rate_limit_increments_attempts() {
    let input = r#"{
        "attempts": 2,
        "first_attempt": 1000,
        "now": 2000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result = rate_limit_check_json(input);
    assert!(result.contains("\"allowed\":true"));
    assert!(result.contains("\"new_attempts\":3"));
    assert!(result.contains("\"remaining_attempts\":2"));
}

#[test]
fn rate_limit_exponential_backoff() {
    let input1 = r#"{
        "attempts": 5,
        "first_attempt": 1000,
        "now": 2000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result1 = rate_limit_check_json(input1);
    assert!(result1.contains("\"allowed\":false"));

    let input2 = r#"{
        "attempts": 6,
        "first_attempt": 1000,
        "now": 2000,
        "max_attempts": 5,
        "window_ms": 60000,
        "blocked_until": null
    }"#;
    let result2 = rate_limit_check_json(input2);
    assert!(result2.contains("\"allowed\":false"));
    // Higher attempts should result in longer backoff
}

#[test]
fn rate_limit_invalid_input_returns_empty() {
    let result = rate_limit_check_json("invalid json");
    assert_eq!(result, "");
}
