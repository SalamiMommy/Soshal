use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

use soshal_nostr_core::models::find_tag_values_map;

#[derive(Deserialize)]
struct SwapEvent {
    #[serde(default)]
    tags: Vec<Vec<String>>,
    content: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    pubkey: String,
    #[serde(default)]
    sig: String,
}

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

/// Verifies the Schnorr signature of a serialized Nostr event. Returns false
/// for malformed JSON, missing signature fields or failed verification.
fn event_signature_valid(event_json: &str) -> bool {
    serde_json::from_str::<nostr::event::Event>(event_json)
        .map(|ev| ev.verify().is_ok())
        .unwrap_or(false)
}

fn validate_swap_event(input: &ValidateSwapInput) -> ValidateSwapOut {
    let event: SwapEvent = match serde_json::from_str(&input.event_json) {
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
    // forged payloads are rejected outright.
    if event.id.is_empty() || event.pubkey.is_empty() || event.sig.is_empty() {
        return ValidateSwapOut {
            valid: false,
            d_tag: None,
            inner_pubkey: None,
        };
    }
    // The signer of the outer event must be the party whose role is being
    // asserted. Without this, an attacker could sign a swap declaring any
    // third party as buyer/seller with their own key.
    if event.pubkey != input.self_pubkey {
        return ValidateSwapOut {
            valid: false,
            d_tag: None,
            inner_pubkey: None,
        };
    }
    if !event_signature_valid(&input.event_json) {
        return ValidateSwapOut {
            valid: false,
            d_tag: None,
            inner_pubkey: None,
        };
    }
    let [d_tag_val, p_tag, role_tag, type_tag] =
        find_tag_values_map(&event.tags, ["d", "p", "role", "type"]);
    let d_tag = d_tag_val.map(|s| s.to_string());
    let d_present = d_tag.is_some();
    let p_ok = p_tag == Some(input.self_pubkey.as_str());
    let role_ok = role_tag == Some(input.expected_role.as_str());
    let type_ok = type_tag == Some(input.expected_type.as_str());
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
        if inner.id.is_empty() || inner.pubkey != input.self_pubkey {
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
    let Some(input) = json_in::<Option<ValidateSwapInput>>(input, None) else {
        return r#"{"valid":false}"#.to_string();
    };
    json_out(&validate_swap_event(&input), r#"{"valid":false}"#)
}
