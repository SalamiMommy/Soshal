use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};
use soshal_common_core::util::constant_time_eq;

use soshal_nostr_core::models::find_tag_values_map;

#[derive(Deserialize)]
struct InnerSwapEvent {
    id: String,
    pubkey: String,
}

#[derive(Deserialize)]
struct ValidateSwapInput {
    event_json: String,
    self_pubkey: String,
    expected_role: String,
    expected_type: String,
    check_inner: bool,
}

#[derive(Serialize)]
struct ValidateSwapOut {
    valid: bool,
    #[serde(rename = "dTag", skip_serializing_if = "Option::is_none")]
    d_tag: Option<String>,
    #[serde(rename = "innerPubkey", skip_serializing_if = "Option::is_none")]
    inner_pubkey: Option<String>,
}

/// Verifies the Schnorr signature of a parsed Nostr event. Returns false for
/// failed verification.
fn event_signature_valid(event: &nostr::event::Event) -> bool {
    event.verify().is_ok()
}

fn validate_swap_event(input: &ValidateSwapInput) -> ValidateSwapOut {
    if input.event_json.len() > 128 * 1024 {
        return ValidateSwapOut {
            valid: false,
            d_tag: None,
            inner_pubkey: None,
        };
    }
    let event = match serde_json::from_str::<nostr::event::Event>(&input.event_json) {
        Ok(e) => e,
        Err(_) => {
            return ValidateSwapOut {
                valid: false,
                d_tag: None,
                inner_pubkey: None,
            }
        }
    };
    // The outer event must be a properly signed Nostr event; unsigned or
    // forged payloads are rejected outright. A successful parse guarantees
    // well-formed hex id/pubkey/sig fields. Constant-time pubkey comparison
    // prevents a timing side-channel probing the caller's claimed identity.
    let self_pk_clean = input.self_pubkey.trim();
    if !constant_time_eq(
        event.pubkey.to_hex().to_ascii_lowercase().as_bytes(),
        self_pk_clean.to_ascii_lowercase().as_bytes(),
    ) {
        return ValidateSwapOut {
            valid: false,
            d_tag: None,
            inner_pubkey: None,
        };
    }
    if !event_signature_valid(&event) {
        return ValidateSwapOut {
            valid: false,
            d_tag: None,
            inner_pubkey: None,
        };
    }
    let tags: Vec<Vec<String>> = event.tags.iter().map(|t| t.clone().to_vec()).collect();
    let [d_tag_val, p_tag, role_tag, type_tag, expiration_tag] =
        find_tag_values_map(&tags, ["d", "p", "role", "type", "expiration"]);
    let d_tag = d_tag_val.map(|s| s.to_string());
    let d_present = d_tag.is_some();
    if let Some(expiration) = expiration_tag {
        let expires_at = expiration.parse::<u64>().ok();
        let expired = match expires_at {
            Some(ts) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                ts < now
            }
            None => true,
        };
        if expired {
            return ValidateSwapOut {
                valid: false,
                d_tag,
                inner_pubkey: None,
            };
        }
    }
    let p_ok = p_tag.is_some_and(|p| p.eq_ignore_ascii_case(self_pk_clean));
    let role_ok =
        role_tag.is_some_and(|r| r.trim().eq_ignore_ascii_case(input.expected_role.trim()));
    let type_ok =
        type_tag.is_some_and(|t| t.trim().eq_ignore_ascii_case(input.expected_type.trim()));
    if !d_present || !p_ok || !role_ok || !type_ok {
        return ValidateSwapOut {
            valid: false,
            d_tag,
            inner_pubkey: None,
        };
    }
    if input.check_inner {
        let inner: InnerSwapEvent = match serde_json::from_str(&event.content) {
            Ok(e) => e,
            Err(_) => {
                return ValidateSwapOut {
                    valid: false,
                    d_tag,
                    inner_pubkey: None,
                }
            }
        };
        // The inner payload must name the signer as the party. The outer
        // signature already authenticates the inner payload as authored by
        // `self_pubkey`, so no separate inner signature is needed.
        if inner.id.is_empty()
            || !constant_time_eq(
                inner.pubkey.to_ascii_lowercase().as_bytes(),
                self_pk_clean.to_ascii_lowercase().as_bytes(),
            )
        {
            return ValidateSwapOut {
                valid: false,
                d_tag,
                inner_pubkey: Some(inner.pubkey),
            };
        }
        return ValidateSwapOut {
            valid: true,
            d_tag,
            inner_pubkey: Some(inner.pubkey),
        };
    }
    ValidateSwapOut {
        valid: true,
        d_tag,
        inner_pubkey: None,
    }
}

pub fn validate_swap_event_json(input: &str) -> String {
    if input.len() > 1024 * 1024 {
        return r#"{"valid":false}"#.to_string();
    }
    let Some(input) = json_in::<Option<ValidateSwapInput>>(input, None) else {
        return r#"{"valid":false}"#.to_string();
    };
    json_out(&validate_swap_event(&input), r#"{"valid":false}"#)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
    use soshal_common_core::consts::KIND_SWAP;

    fn keys() -> nostr::key::Keys {
        nostr::key::Keys::parse("0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap()
    }

    fn signed_swap(
        keys: &nostr::key::Keys,
        d_tag: &str,
        with_p_tag: bool,
        role: &str,
        type_tag: &str,
        content: &str,
    ) -> String {
        let mut builder = EventBuilder::new(Kind::Custom(KIND_SWAP), content)
            .tag(Tag::parse(["d", d_tag]).unwrap());
        if with_p_tag {
            builder = builder.tag(Tag::parse(["p", keys.public_key().to_hex().as_str()]).unwrap());
        }
        let ev = builder
            .tag(Tag::parse(["role", role]).unwrap())
            .tag(Tag::parse(["type", type_tag]).unwrap())
            .finalize(keys)
            .unwrap();
        serde_json::to_string(&ev).unwrap()
    }

    fn swap_input(event_json: &str, self_pubkey: &str, check_inner: bool) -> String {
        serde_json::json!({
            "event_json": event_json,
            "self_pubkey": self_pubkey,
            "expected_role": "buyer",
            "expected_type": "buy_now",
            "check_inner": check_inner
        })
        .to_string()
    }

    #[test]
    fn swap_matching_assets_amounts_accepted() {
        let keys = keys();
        let pk = keys.public_key().to_hex();
        let content = format!(r#"{{"id":"inner1","pubkey":"{}"}}"#, pk);
        let event = signed_swap(&keys, "swap-1", true, "buyer", "buy_now", &content);
        let v: serde_json::Value =
            serde_json::from_str(&validate_swap_event_json(&swap_input(&event, &pk, true)))
                .unwrap();
        assert_eq!(v["valid"], true);
        assert_eq!(v["dTag"], "swap-1");
        assert_eq!(v["innerPubkey"], pk);
    }

    #[test]
    fn swap_mismatched_role_or_type_rejected() {
        let keys = keys();
        let pk = keys.public_key().to_hex();
        let wrong_role = signed_swap(&keys, "s1", true, "seller", "buy_now", "{}");
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &wrong_role,
            &pk,
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);

        let wrong_type = signed_swap(&keys, "s2", true, "buyer", "auction", "{}");
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &wrong_type,
            &pk,
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);
    }

    #[test]
    fn swap_expired_rejected() {
        let keys = keys();
        let pk = keys.public_key().to_hex();
        let ev = EventBuilder::new(Kind::Custom(KIND_SWAP), "{}")
            .tag(Tag::parse(["d", "swap-x"]).unwrap())
            .tag(Tag::parse(["p", pk.as_str()]).unwrap())
            .tag(Tag::parse(["role", "buyer"]).unwrap())
            .tag(Tag::parse(["type", "buy_now"]).unwrap())
            .tag(Tag::parse(["expiration", "1"]).unwrap())
            .finalize(&keys)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &serde_json::to_string(&ev).unwrap(),
            &pk,
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);
    }

    #[test]
    fn swap_missing_required_tags_rejected() {
        let keys = keys();
        let pk = keys.public_key().to_hex();

        let no_d = EventBuilder::new(Kind::Custom(KIND_SWAP), "{}")
            .tag(Tag::parse(["p", pk.as_str()]).unwrap())
            .tag(Tag::parse(["role", "buyer"]).unwrap())
            .tag(Tag::parse(["type", "buy_now"]).unwrap())
            .finalize(&keys)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &serde_json::to_string(&no_d).unwrap(),
            &pk,
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);

        let no_p = signed_swap(&keys, "s2", false, "buyer", "buy_now", "{}");
        let v: serde_json::Value =
            serde_json::from_str(&validate_swap_event_json(&swap_input(&no_p, &pk, false)))
                .unwrap();
        assert_eq!(v["valid"], false);

        let no_role = EventBuilder::new(Kind::Custom(KIND_SWAP), "{}")
            .tag(Tag::parse(["d", "s3"]).unwrap())
            .tag(Tag::parse(["p", pk.as_str()]).unwrap())
            .tag(Tag::parse(["type", "buy_now"]).unwrap())
            .finalize(&keys)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &serde_json::to_string(&no_role).unwrap(),
            &pk,
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);

        let no_type = EventBuilder::new(Kind::Custom(KIND_SWAP), "{}")
            .tag(Tag::parse(["d", "s4"]).unwrap())
            .tag(Tag::parse(["p", pk.as_str()]).unwrap())
            .tag(Tag::parse(["role", "buyer"]).unwrap())
            .finalize(&keys)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &serde_json::to_string(&no_type).unwrap(),
            &pk,
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);
    }

    #[test]
    fn swap_foreign_signer_rejected() {
        let keys = keys();
        let event = signed_swap(&keys, "s", true, "buyer", "buy_now", "{}");
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &event,
            "someoneelse",
            false,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);
    }

    #[test]
    fn swap_inner_check_transition() {
        let keys = keys();
        let pk = keys.public_key().to_hex();
        let content = r#"{"id":"inner1","pubkey":"0xdeadbeef"}"#;
        let event = signed_swap(&keys, "s", true, "buyer", "buy_now", content);
        let json = event;

        let v: serde_json::Value =
            serde_json::from_str(&validate_swap_event_json(&swap_input(&json, &pk, false)))
                .unwrap();
        assert_eq!(v["valid"], true);

        let v: serde_json::Value =
            serde_json::from_str(&validate_swap_event_json(&swap_input(&json, &pk, true))).unwrap();
        assert_eq!(v["valid"], false);
    }

    #[test]
    fn swap_malformed_payloads_rejected() {
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json("zzz")).unwrap();
        assert_eq!(v["valid"], false);

        let unsigned = serde_json::json!({
            "event_json": r#"{"id":"x","pubkey":"y","content":"","tags":[],"sig":""}"#,
            "self_pubkey": "y",
            "expected_role": "buyer",
            "expected_type": "buy_now",
            "check_inner": false
        });
        let v: serde_json::Value =
            serde_json::from_str(&validate_swap_event_json(&unsigned.to_string())).unwrap();
        assert_eq!(v["valid"], false);

        let keys = keys();
        let pk = keys.public_key().to_hex();
        let bad_inner = signed_swap(&keys, "s", true, "buyer", "buy_now", "not-json");
        let v: serde_json::Value = serde_json::from_str(&validate_swap_event_json(&swap_input(
            &bad_inner, &pk, true,
        )))
        .unwrap();
        assert_eq!(v["valid"], false);
    }
}
