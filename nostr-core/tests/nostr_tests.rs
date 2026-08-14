//! Integration tests for soshal-nostr-core.

use nostr::nips::nip19::ToBech32;
use soshal_nostr_core::events::{filter, filter_authors, filter_kinds, text_note};
use soshal_nostr_core::keys::{from_nsec, from_sk, generate_keys};
use soshal_nostr_core::models::{verified_event_from_value, NostrEvent};

const SK_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const PK_HEX_ONE: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[test]
fn generate_keys_produces_usable_keys() {
    let keys = generate_keys();
    let sk = keys.secret_key().to_secret_hex();
    assert_eq!(sk.len(), 64);
    assert!(keys.public_key().to_hex().len() == 64);
}

#[test]
fn from_sk_parses_hex_private_key() {
    let keys = from_sk(SK_HEX).unwrap();
    assert_eq!(keys.public_key().to_hex(), PK_HEX_ONE);
    assert_eq!(keys.secret_key().to_secret_hex(), SK_HEX);
}

#[test]
fn from_nsec_matches_hex_private_key() {
    let keys = from_sk(SK_HEX).unwrap();
    let nsec = keys.secret_key().to_bech32().unwrap();
    assert!(nsec.starts_with("nsec1"));
    let parsed = from_nsec(&nsec).unwrap();
    assert_eq!(parsed.secret_key().to_secret_hex(), SK_HEX);
    assert_eq!(parsed.public_key().to_hex(), PK_HEX_ONE);
}

#[test]
fn invalid_keys_rejected() {
    assert!(from_sk("not-a-key").is_err());
    assert!(from_nsec("nsec1invalid").is_err());
}

#[test]
fn text_note_signs_verifiable_event() {
    let keys = from_sk(SK_HEX).unwrap();
    let event = text_note(&keys, "hello nostr", vec![]).unwrap();
    assert_eq!(event.kind, nostr::event::Kind::TextNote);
    assert_eq!(event.content, "hello nostr");
    assert_eq!(event.pubkey.to_hex(), PK_HEX_ONE);
    assert!(event.verify().is_ok());
}

#[test]
fn text_note_includes_tags() {
    let keys = from_sk(SK_HEX).unwrap();
    let other = from_sk("02".repeat(32).as_str()).unwrap();
    let event = text_note(
        &keys,
        "mention",
        vec![nostr::event::Tag::public_key(other.public_key())],
    )
    .unwrap();
    let p_tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .filter(|t| t.as_slice()[0] == "p")
        .map(|t| t.as_slice().to_vec())
        .collect();
    assert_eq!(p_tags.len(), 1);
    assert!(event.verify().is_ok());
}

#[test]
fn nostr_event_from_event_maps_fields() {
    let keys = from_sk(SK_HEX).unwrap();
    let event = text_note(&keys, "content here", vec![]).unwrap();
    let ours: NostrEvent = NostrEvent::from(&event);
    assert_eq!(ours.id, event.id.to_hex());
    assert_eq!(ours.pubkey, PK_HEX_ONE);
    assert_eq!(ours.content, "content here");
    assert_eq!(ours.kind, 1);
    assert!(ours.created_at > 0.0);
}

#[test]
fn serde_roundtrip_of_nostr_event_json() {
    let keys = from_sk(SK_HEX).unwrap();
    let event = text_note(&keys, "serde", vec![]).unwrap();
    let json = serde_json::to_value(&event).unwrap();
    let ours: NostrEvent = serde_json::from_value(json).unwrap();
    assert_eq!(ours.id, event.id.to_hex());
    assert_eq!(ours.created_at, event.created_at.as_secs() as f64);
    assert_eq!(ours.kind, 1);
}

#[test]
fn verified_event_from_value_accepts_valid_signed_event() {
    let keys = from_sk(SK_HEX).unwrap();
    let event = text_note(&keys, "signed", vec![]).unwrap();
    let value = serde_json::to_value(&event).unwrap();
    let verified = verified_event_from_value(value.clone());
    assert!(verified.is_some());
    assert_eq!(verified.unwrap().id, event.id);
}

#[test]
fn verified_event_from_value_rejects_tampered_content() {
    let keys = from_sk(SK_HEX).unwrap();
    let event = text_note(&keys, "original", vec![]).unwrap();
    let mut value = serde_json::to_value(&event).unwrap();
    value["content"] = serde_json::json!("tampered");
    assert!(verified_event_from_value(value).is_none());
}

#[test]
fn verified_event_from_value_rejects_tampered_signature() {
    let keys = from_sk(SK_HEX).unwrap();
    let event = text_note(&keys, "original", vec![]).unwrap();
    let mut value = serde_json::to_value(&event).unwrap();
    let sig = value["sig"].as_str().unwrap();
    let flipped: String = sig
        .chars()
        .map(|c| if c == 'a' { 'b' } else { 'a' })
        .take(sig.len())
        .collect();
    value["sig"] = serde_json::json!(flipped);
    assert!(verified_event_from_value(value).is_none());
}

#[test]
fn verified_event_from_value_rejects_garbage() {
    assert!(verified_event_from_value(serde_json::json!({"nope": 1})).is_none());
    assert!(verified_event_from_value(serde_json::Value::Null).is_none());
}

#[test]
fn filter_builders_build_valid_filters() {
    let f = filter();
    assert!(f.kinds.is_none());
    assert!(f.authors.is_none());

    let fk = filter_kinds(vec![nostr::event::Kind::TextNote]);
    let kinds: Vec<nostr::event::Kind> = fk.kinds.unwrap().into_iter().collect();
    assert_eq!(kinds, vec![nostr::event::Kind::TextNote]);

    let keys = from_sk(SK_HEX).unwrap();
    let fa = filter_authors(vec![keys.public_key()]);
    assert_eq!(fa.authors.unwrap().len(), 1);
}

#[test]
fn nostr_event_find_tag_value() {
    use soshal_nostr_core::models::find_tag_value;

    let tags = vec![
        vec!["d".into(), "my-tag-val".into()],
        vec!["p".into(), "pubkey-val".into()],
    ];
    assert_eq!(find_tag_value(&tags, "d"), Some("my-tag-val"));
    assert_eq!(find_tag_value(&tags, "p"), Some("pubkey-val"));
    assert_eq!(find_tag_value(&tags, "nonexistent"), None);
}
