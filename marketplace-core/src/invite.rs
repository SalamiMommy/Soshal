use soshal_common_core::json_util::{json_in, json_out};

#[derive(serde::Deserialize)]
struct ValidateInviteInput {
    #[serde(rename = "expiresAt")]
    expires_at: f64,
    #[serde(rename = "maxUses")]
    max_uses: f64,
    uses: f64,
    #[serde(rename = "nowMs")]
    now_ms: f64,
}

#[derive(serde::Serialize)]
pub struct ValidateInviteOut {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn validate_invite(input: &ValidateInviteInput) -> ValidateInviteOut {
    if !input.expires_at.is_finite()
        || !input.max_uses.is_finite()
        || !input.uses.is_finite()
        || !input.now_ms.is_finite()
    {
        return ValidateInviteOut {
            valid: false,
            reason: Some("invalid_input".to_string()),
        };
    }
    if input.expires_at < 0.0 || input.max_uses < 0.0 || input.uses < 0.0 || input.now_ms < 0.0 {
        return ValidateInviteOut {
            valid: false,
            reason: Some("invalid_input".to_string()),
        };
    }
    if input.expires_at > 0.0 && input.now_ms > input.expires_at {
        return ValidateInviteOut {
            valid: false,
            reason: Some("expired".to_string()),
        };
    }
    if input.max_uses > 0.0 && input.uses >= input.max_uses {
        return ValidateInviteOut {
            valid: false,
            reason: Some("max_uses".to_string()),
        };
    }
    ValidateInviteOut {
        valid: true,
        reason: None,
    }
}

pub fn validate_invite_json(input: &str) -> String {
    let Some(parsed) = json_in::<Option<ValidateInviteInput>>(input, None) else {
        return r#"{"valid":false,"reason":"parse_error"}"#.to_string();
    };
    json_out(&validate_invite(&parsed), r#"{"valid":false}"#)
}
