//! Integration tests for soshal-marketplace-core.

use soshal_marketplace_core::calendar::parse_calendar_event_json;
use soshal_marketplace_core::escrow::{can_release, EscrowStatus};
use soshal_marketplace_core::invite::validate_invite_json;
use soshal_marketplace_core::listing::parse_listing_json;
use soshal_marketplace_core::listing::parse_listing_value;
use soshal_marketplace_core::poll::parse_poll_event_json;
use soshal_marketplace_core::swap::validate_swap_event_json;

// ---------------------------------------------------------------------------
// Escrow
// ---------------------------------------------------------------------------

#[test]
fn escrow_can_release_requires_both_without_dispute() {
    assert!(!can_release(false, false, false, false));
    assert!(!can_release(true, false, false, false));
    assert!(!can_release(false, true, false, false));
    assert!(can_release(true, true, false, false));
}

#[test]
fn escrow_dispute_needs_arbitrator_approval() {
    assert!(!can_release(true, true, false, true));
    assert!(!can_release(false, false, false, true));
    assert!(can_release(false, false, true, true));
    assert!(can_release(true, true, true, true));
}

#[test]
fn escrow_status_serializes_pascal_case() {
    assert_eq!(
        serde_json::to_string(&EscrowStatus::Pending).unwrap(),
        r#""Pending""#
    );
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

fn listing_input() -> serde_json::Value {
    serde_json::json!({
        "id": "ev1",
        "pubkey": "pk1",
        "content": r#"{"description":"great sofa","condition":"new","contactMethods":["signal:abc"],"escrowEnabled":true}"#,
        "created_at": 1000.0,
        "tags": [
            ["d", "sofa-1"],
            ["title", "Blue Sofa"],
            ["price", "120.5"],
            ["currency", "EUR"],
            ["location", "u33dc"],
            ["image", "https://x/a.png"],
            ["video", "https://x/a.mp4"],
            ["t", "furniture"]
        ]
    })
}

#[test]
fn parse_listing_maps_tags_and_content() {
    let out = parse_listing_json(&listing_input().to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["id"], "ev1");
    assert_eq!(v["dTag"], "sofa-1");
    assert_eq!(v["title"], "Blue Sofa");
    assert_eq!(v["price"], 120.5);
    assert_eq!(v["currency"], "EUR");
    assert_eq!(v["condition"], "new");
    assert_eq!(v["description"], "great sofa");
    assert_eq!(v["locationGeohash"], "u33dc");
    assert_eq!(v["images"][0], "https://x/a.png");
    assert_eq!(v["videos"][0], "https://x/a.mp4");
    assert_eq!(v["contactMethods"][0], "signal:abc");
    assert_eq!(v["tags"][0], "furniture");
    assert_eq!(v["createdAt"], 1000.0);
    assert_eq!(v["escrowEnabled"], true);
}

#[test]
fn parse_listing_falls_back_and_defaults() {
    let input = serde_json::json!({
        "id": "shortid",
        "pubkey": "pk2",
        "content": "",
        "created_at": 5.0,
        "tags": [["title", "Cheap Thing"], ["price", "10"]]
    });
    let out = parse_listing_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["dTag"], "shortid");
    assert_eq!(v["currency"], "USD");
    assert_eq!(v["condition"], "good");
    assert_eq!(v["escrowEnabled"], false);
    assert!(v.get("description").is_none());
    assert!(v.get("videos").is_none());
}

#[test]
fn parse_listing_rejects_invalid_price() {
    let mut input = listing_input();
    input["tags"] = serde_json::json!([["d", "s"], ["title", "t"], ["price", "abc"]]);
    assert_eq!(parse_listing_json(&input.to_string()), "null");

    let mut input2 = listing_input();
    input2["tags"] = serde_json::json!([["d", "s"], ["title", "t"], ["price", "1e20"]]);
    assert_eq!(parse_listing_json(&input2.to_string()), "null");
}

#[test]
fn parse_listing_rejects_missing_price() {
    let input = serde_json::json!({
        "id": "x", "pubkey": "pk", "content": "", "created_at": 1.0,
        "tags": [["title", "no price"]]
    });
    assert_eq!(parse_listing_json(&input.to_string()), "null");
}

#[test]
fn parse_listing_plain_content_becomes_description() {
    let input = serde_json::json!({
        "id": "x", "pubkey": "pk", "content": "plain description", "created_at": 1.0,
        "tags": [["price", "5"]]
    });
    let out = parse_listing_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["description"], "plain description");
}

#[test]
fn parse_listing_value_matches_json_path() {
    let input = listing_input();
    let via_value = parse_listing_value(input.clone());
    let via_json = parse_listing_json(&input.to_string());
    assert_eq!(via_value, via_json, "value path mirrors json string path");
    let v: serde_json::Value = serde_json::from_str(&via_value).unwrap();
    assert_eq!(v["title"], "Blue Sofa");
    assert_eq!(v["price"], 120.5);

    assert_eq!(parse_listing_value(serde_json::Value::Null), "null");
    assert_eq!(
        parse_listing_value(serde_json::json!({"id": "partial"})),
        "null"
    );
    let mut bad = listing_input();
    bad["tags"] = serde_json::json!([["d", "s"], ["title", "t"], ["price", "not-a-price"]]);
    assert_eq!(parse_listing_value(bad), "null");
}

// ---------------------------------------------------------------------------
// Poll
// ---------------------------------------------------------------------------

#[test]
fn parse_poll_event_from_content() {
    let input = serde_json::json!({
        "event": {
            "id": "p1",
            "pubkey": "pk",
            "content": r#"{"question":"best?","options":[{"id":1,"text":"a"},{"id":2,"text":"b"}]}"#,
            "created_at": 1000.0,
            "tags": [["expiration", "2000"]]
        },
        "nowMs": 1500.0
    });
    let out = parse_poll_event_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["question"], "best?");
    assert_eq!(v["options"][0]["text"], "a");
    assert_eq!(v["options"][1]["id"], 2);
    assert_eq!(v["expiresAt"], 2_000_000.0);
    assert_eq!(v["closed"], false);
    assert_eq!(v["createdAt"], 1_000_000.0);
}

#[test]
fn parse_poll_event_falls_back_to_tags_and_default_expiry() {
    let input = serde_json::json!({
        "event": {
            "id": "p2",
            "pubkey": "pk",
            "content": "question text",
            "created_at": 50.0,
            "tags": [["poll_option", "1", "yes"], ["poll_option", "2", "no"]]
        },
        "nowMs": 1000.0
    });
    let out = parse_poll_event_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["question"], "question text");
    assert_eq!(v["options"].as_array().unwrap().len(), 2);
    assert_eq!(v["expiresAt"], 1000.0 + 604800.0 * 1000.0);
    assert_eq!(v["closed"], false);
}

#[test]
fn parse_poll_event_closed_when_expired() {
    let input = serde_json::json!({
        "event": {
            "id": "p3",
            "pubkey": "pk",
            "content": "",
            "created_at": 1.0,
            "tags": [["expiration", "100"]]
        },
    "nowMs": 200000.0
    });
    let out = parse_poll_event_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["closed"], true);
}

#[test]
fn parse_poll_event_rejects_garbage() {
    assert_eq!(parse_poll_event_json("nonsense"), "null");
}

// ---------------------------------------------------------------------------
// Calendar
// ---------------------------------------------------------------------------

#[test]
fn parse_calendar_event_maps_fields() {
    let input = serde_json::json!({
        "id": "c1",
        "pubkey": "pk",
        "content": r#"{"description":"party","image":"https://x/img.png","videos":["https://x/v.mp4"]}"#,
        "created_at": 100.0,
        "tags": [
            ["d", "ev-1"],
            ["title", "Launch"],
            ["start", "1700000000"],
            ["end", "1700003600"],
            ["location", "Berlin"],
            ["p", "participant1"],
            ["p", "participant2"]
        ]
    });
    let out = parse_calendar_event_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["dTag"], "ev-1");
    assert_eq!(v["title"], "Launch");
    assert_eq!(v["startTime"], 1700000000.0);
    assert_eq!(v["endTime"], 1700003600.0);
    assert_eq!(v["location"], "Berlin");
    assert_eq!(v["description"], "party");
    assert_eq!(v["image"], "https://x/img.png");
    assert_eq!(v["videos"][0], "https://x/v.mp4");
    assert_eq!(v["participants"].as_array().unwrap().len(), 2);
    assert_eq!(v["createdAt"], 100.0);
}

#[test]
fn parse_calendar_event_defaults_and_missing_end() {
    let input = serde_json::json!({
        "id": "c2",
        "pubkey": "pk",
        "content": "",
        "created_at": 5.0,
        "tags": [["start", "123"]]
    });
    let out = parse_calendar_event_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["dTag"], "c2");
    assert_eq!(v["title"], "Untitled Event");
    assert!(v.get("endTime").is_none());
    assert!(v.get("videos").is_none());
}

#[test]
fn parse_calendar_event_rejects_missing_invalid_start() {
    let input = serde_json::json!({
        "id": "c3", "pubkey": "pk", "content": "", "created_at": 1.0, "tags": []
    });
    assert_eq!(parse_calendar_event_json(&input.to_string()), "null");

    let mut input2 = input;
    input2["tags"] = serde_json::json!([["start", "notanumber"]]);
    assert_eq!(parse_calendar_event_json(&input2.to_string()), "null");
}

// ---------------------------------------------------------------------------
// Invite
// ---------------------------------------------------------------------------

#[test]
fn validate_invite_valid() {
    let input = serde_json::json!({
        "expiresAt": 1000.0, "maxUses": 10.0, "uses": 2.0, "nowMs": 500.0
    });
    let out = validate_invite_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["valid"], true);
    assert!(v.get("reason").is_none());
}

#[test]
fn validate_invite_expired() {
    let input = serde_json::json!({
        "expiresAt": 1000.0, "maxUses": 10.0, "uses": 2.0, "nowMs": 2000.0
    });
    let v: serde_json::Value =
        serde_json::from_str(&validate_invite_json(&input.to_string())).unwrap();
    assert_eq!(v["valid"], false);
    assert_eq!(v["reason"], "expired");
}

#[test]
fn validate_invite_max_uses_reached() {
    let input = serde_json::json!({
        "expiresAt": 0.0, "maxUses": 3.0, "uses": 3.0, "nowMs": 0.0
    });
    let v: serde_json::Value =
        serde_json::from_str(&validate_invite_json(&input.to_string())).unwrap();
    assert_eq!(v["valid"], false);
    assert_eq!(v["reason"], "max_uses");
}

#[test]
fn validate_invite_invalid_input() {
    let input = serde_json::json!({
        "expiresAt": -1.0, "maxUses": 3.0, "uses": 0.0, "nowMs": 0.0
    });
    let v: serde_json::Value =
        serde_json::from_str(&validate_invite_json(&input.to_string())).unwrap();
    assert_eq!(v["valid"], false);
    assert_eq!(v["reason"], "invalid_input");

    let v2: serde_json::Value = serde_json::from_str(&validate_invite_json("garbage")).unwrap();
    assert_eq!(v2["valid"], false);
    assert_eq!(v2["reason"], "parse_error");
}

#[test]
fn validate_invite_zero_limits_never_expire() {
    let input = serde_json::json!({
        "expiresAt": 0.0, "maxUses": 0.0, "uses": 999.0, "nowMs": 999999.0
    });
    let v: serde_json::Value =
        serde_json::from_str(&validate_invite_json(&input.to_string())).unwrap();
    assert_eq!(v["valid"], true);
}

// ---------------------------------------------------------------------------
// Swap
// ---------------------------------------------------------------------------

fn swap_event_json(
    keys: &nostr::key::Keys,
    d_tag: &str,
    with_p_tag: bool,
    content: &str,
) -> String {
    let mut builder = nostr::event::EventBuilder::new(nostr::event::Kind::Custom(38383), content)
        .tag(nostr::event::Tag::parse(["d", d_tag]).unwrap());
    if with_p_tag {
        builder = builder
            .tag(nostr::event::Tag::parse(["p", keys.public_key().to_hex().as_str()]).unwrap());
    }
    use nostr::event::FinalizeEvent;
    let ev = builder
        .tag(nostr::event::Tag::parse(["role", "buyer"]).unwrap())
        .tag(nostr::event::Tag::parse(["type", "buy_now"]).unwrap())
        .finalize(keys)
        .unwrap();
    serde_json::to_string(&ev).unwrap()
}

#[test]
fn validate_swap_accepts_well_formed_signed_event() {
    let keys =
        nostr::key::Keys::parse("0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
    let inner_pk = keys.public_key().to_hex();
    let content = format!(r#"{{"id":"inner1","pubkey":"{}"}}"#, inner_pk);
    let event_json = swap_event_json(&keys, "swap-1", true, &content);
    let input = serde_json::json!({
        "event_json": event_json,
        "self_pubkey": inner_pk,
        "expected_role": "buyer",
        "expected_type": "buy_now",
        "check_inner": true
    });
    let out = validate_swap_event_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["valid"], true);
    assert_eq!(v["dTag"], "swap-1");
    assert_eq!(v["innerPubkey"], inner_pk);
}

#[test]
fn validate_swap_rejects_foreign_signer() {
    let keys =
        nostr::key::Keys::parse("0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
    let content = format!(
        r#"{{"id":"inner1","pubkey":"{}"}}"#,
        keys.public_key().to_hex()
    );
    let event_json = swap_event_json(&keys, "swap-1", true, &content);
    let input = serde_json::json!({
        "event_json": event_json,
        "self_pubkey": "someoneelse",
        "expected_role": "buyer",
        "expected_type": "buy_now",
        "check_inner": false
    });
    let v: serde_json::Value =
        serde_json::from_str(&validate_swap_event_json(&input.to_string())).unwrap();
    assert_eq!(v["valid"], false);
}

#[test]
fn validate_swap_rejects_unsigned_or_garbage() {
    let input = serde_json::json!({
        "event_json": r#"{"id":"x","pubkey":"y","content":"","tags":[],"sig":""}"#,
        "self_pubkey": "y",
        "expected_role": "buyer",
        "expected_type": "buy_now",
        "check_inner": false
    });
    let v: serde_json::Value =
        serde_json::from_str(&validate_swap_event_json(&input.to_string())).unwrap();
    assert_eq!(v["valid"], false);

    let v2: serde_json::Value = serde_json::from_str(&validate_swap_event_json("zzz")).unwrap();
    assert_eq!(v2["valid"], false);
}

#[test]
fn parse_listing_currency_and_location_tag_variations() {
    // Too long currency tag rejected
    let input = serde_json::json!({
        "id": "x", "pubkey": "pk", "content": "", "created_at": 1.0,
        "tags": [["price", "10"], ["currency", "SUPERLONGROUNDCURRENCYSTRING"]]
    });
    assert_eq!(parse_listing_json(&input.to_string()), "null");

    // Location tag using 'g' tag fallback
    let input2 = serde_json::json!({
        "id": "x2", "pubkey": "pk", "content": "", "created_at": 1.0,
        "tags": [["price", "10"], ["g", "u33dc5"]]
    });
    let out = parse_listing_json(&input2.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["locationGeohash"], "u33dc5");
}
