use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

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
    let is_admin_or_owner = input.role == "owner" || input.role == "admin";
    let bitmask_allowed = input.required_bit == 0
        || (input.permissions_mask & input.required_bit) == input.required_bit;
    let allowed = is_admin_or_owner || bitmask_allowed;
    GroupPermissionOutput { allowed }
}

/// JSON-string entry point for [`build_group_message_envelope`]. Returns
/// `"{}"` on malformed input.
pub fn build_group_message_envelope_json(input: &str) -> String {
    let Some(i) = json_in::<Option<BuildGroupMsgEnvelopeInput>>(input, None) else {
        return "{}".to_string();
    };
    json_out(&build_group_message_envelope(i.pqc_ct, i.payload), "{}")
}

/// JSON-string entry point for [`validate_group_permissions`]. Returns
/// `{"allowed":false}` on malformed input.
pub fn validate_group_permissions_json(input: &str) -> String {
    let Some(i) = json_in::<Option<GroupPermissionInput>>(input, None) else {
        return r#"{"allowed":false}"#.to_string();
    };
    json_out(&validate_group_permissions(&i), r#"{"allowed":false}"#)
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
