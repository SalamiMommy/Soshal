use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::json_out;

// ─── Build Group Message Envelope ──────────────────────────────────────────

/// Input for [`build_group_message_envelope`].
#[derive(Deserialize)]
pub struct BuildGroupMsgEnvelopeInput {
    #[serde(rename = "pqcCt")]
    pub pqc_ct: String,
    pub payload: String,
}

/// Output of [`build_group_message_envelope`].
#[derive(Serialize, Deserialize)]
pub struct GroupMsgEnvelopeOutput {
    #[serde(rename = "pqc_ct")]
    pub pqc_ct: String,
    pub payload: String,
}

pub fn build_group_message_envelope(pqc_ct: String, payload: String) -> GroupMsgEnvelopeOutput {
    GroupMsgEnvelopeOutput { pqc_ct, payload }
}

// ─── Group Permission Validation ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct GroupPermissionInput {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub permissions_mask: u32,
    #[serde(default)]
    pub required_bit: u32,
}

#[derive(Serialize)]
pub struct GroupPermissionOutput {
    pub allowed: bool,
}

pub fn validate_group_permissions(input: &GroupPermissionInput) -> GroupPermissionOutput {
    let is_admin_or_owner = input.role.trim().eq_ignore_ascii_case("owner")
        || input.role.trim().eq_ignore_ascii_case("admin");
    let bitmask_allowed = input.required_bit == 0
        || (input.permissions_mask & input.required_bit) == input.required_bit;
    let allowed = is_admin_or_owner || bitmask_allowed;
    GroupPermissionOutput { allowed }
}

#[derive(Deserialize)]
struct BuildGroupMsgEnvelopeInputBorrow<'a> {
    #[serde(borrow, rename = "pqcCt")]
    pqc_ct: &'a str,
    #[serde(borrow)]
    payload: &'a str,
}

#[derive(Serialize)]
struct GroupMsgEnvelopeOutputBorrow<'a> {
    pqc_ct: &'a str,
    payload: &'a str,
}

#[derive(Deserialize)]
struct GroupPermissionInputBorrow<'a> {
    #[serde(borrow, default)]
    role: &'a str,
    #[serde(default)]
    permissions_mask: u32,
    #[serde(default)]
    required_bit: u32,
}

/// JSON-string entry point for [`build_group_message_envelope`]. Returns
/// `"{}"` on malformed input.
pub fn build_group_message_envelope_json(input: &str) -> String {
    if input.len() > 1024 * 1024 {
        return "{}".to_string();
    }
    let Some(i) =
        soshal_common_core::json_util::json_in_borrow::<BuildGroupMsgEnvelopeInputBorrow>(input)
    else {
        return "{}".to_string();
    };
    json_out(
        &GroupMsgEnvelopeOutputBorrow {
            pqc_ct: i.pqc_ct,
            payload: i.payload,
        },
        "{}",
    )
}

/// JSON-string entry point for [`validate_group_permissions`]. Returns
/// `{"allowed":false}` on malformed input.
pub fn validate_group_permissions_json(input: &str) -> String {
    if input.len() > 1024 * 1024 {
        return r#"{"allowed":false}"#.to_string();
    }
    let Some(i) =
        soshal_common_core::json_util::json_in_borrow::<GroupPermissionInputBorrow>(input)
    else {
        return r#"{"allowed":false}"#.to_string();
    };
    let is_admin_or_owner =
        i.role.trim().eq_ignore_ascii_case("owner") || i.role.trim().eq_ignore_ascii_case("admin");
    let bitmask_allowed =
        i.required_bit == 0 || (i.permissions_mask & i.required_bit) == i.required_bit;
    let allowed = is_admin_or_owner || bitmask_allowed;
    json_out(&GroupPermissionOutput { allowed }, r#"{"allowed":false}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_group_message_envelope_valid() {
        let out = build_group_message_envelope("ct1".to_string(), "payload1".to_string());
        assert_eq!(out.pqc_ct, "ct1");
        assert_eq!(out.payload, "payload1");
    }

    #[test]
    fn build_group_message_envelope_json_roundtrip() {
        let input = r#"{"pqcCt":"ct1","payload":"payload1"}"#;
        let out = build_group_message_envelope_json(input);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["pqc_ct"], "ct1");
        assert_eq!(v["payload"], "payload1");
        let back: GroupMsgEnvelopeOutput = serde_json::from_str(&out).unwrap();
        assert_eq!(back.pqc_ct, "ct1");
        assert_eq!(back.payload, "payload1");
    }

    #[test]
    fn build_group_message_envelope_missing_or_empty_input() {
        assert_eq!(build_group_message_envelope_json(r#"{}"#), "{}");
        assert_eq!(
            build_group_message_envelope_json(r#"{"pqcCt":"ct1"}"#),
            "{}"
        );
        assert_eq!(
            build_group_message_envelope_json(r#"{"payload":"p"}"#),
            "{}"
        );
        assert_eq!(build_group_message_envelope_json("garbage"), "{}");
        assert_eq!(build_group_message_envelope_json(""), "{}");
        let out = build_group_message_envelope(String::new(), String::new());
        assert_eq!(out.pqc_ct, "");
        assert_eq!(out.payload, "");
    }
}
