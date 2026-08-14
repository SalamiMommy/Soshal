use serde::{Deserialize, Serialize};

// ─── Build Key Distribution Content ────────────────────────────────────────

/// Input for [`build_key_distribution_content`].
#[derive(Deserialize)]
pub struct BuildKeyDistInput {
    #[serde(rename = "groupId", default)]
    pub group_id: String,
    #[serde(rename = "sharedKey", default)]
    pub shared_key: String,
    #[serde(rename = "sharedPubkey", default)]
    pub shared_pubkey: String,
    #[serde(rename = "pqcCt", default)]
    pub pqc_ct: String,
}

/// Output of [`build_key_distribution_content`].
///
/// SECURITY: the plaintext group `sharedKey` must never be serialized into a
/// key-distribution event; only per-recipient KEM ciphertexts (`pqc_ct`) are
/// published. The shared key travels inside the ciphertext, never in the clear.
#[derive(Serialize)]
pub struct KeyDistContentOutput {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "sharedPubkey")]
    pub shared_pubkey: String,
    #[serde(rename = "pqc_ct", skip_serializing_if = "String::is_empty")]
    pub pqc_ct: String,
}

pub fn build_key_distribution_content(input: &BuildKeyDistInput) -> KeyDistContentOutput {
    let _ = input.shared_key.as_str();
    KeyDistContentOutput {
        group_id: input.group_id.clone(),
        shared_pubkey: input.shared_pubkey.clone(),
        pqc_ct: input.pqc_ct.clone(),
    }
}

// ─── Parse Key Distribution Content ────────────────────────────────────────

/// Parsed key distribution content.
///
/// SECURITY: a legacy event may carry a plaintext `sharedKey`; it is parsed
/// out of the wire JSON but NEVER surfaced in this output — a plaintext
/// group key from an event must not reach the webview or any caller.
#[derive(Serialize)]
pub struct ParsedKeyDistContent {
    #[serde(rename = "valid")]
    pub valid: bool,
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "sharedPubkey")]
    pub shared_pubkey: String,
    #[serde(rename = "pqcCt")]
    pub pqc_ct: String,
    #[serde(rename = "errorReason", skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
}

pub fn parse_key_distribution_content(content: &str) -> ParsedKeyDistContent {
    let fallback = || ParsedKeyDistContent {
        valid: false,
        group_id: String::new(),
        shared_pubkey: String::new(),
        pqc_ct: String::new(),
        error_reason: Some("Invalid JSON".to_string()),
    };

    let parsed: BuildKeyDistInput = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return fallback(),
    };

    if parsed.group_id.is_empty() {
        return ParsedKeyDistContent {
            valid: false,
            group_id: String::new(),
            shared_pubkey: String::new(),
            pqc_ct: String::new(),
            error_reason: Some("Missing groupId".to_string()),
        };
    }
    if parsed.pqc_ct.is_empty() {
        return ParsedKeyDistContent {
            valid: false,
            group_id: parsed.group_id,
            shared_pubkey: parsed.shared_pubkey,
            pqc_ct: String::new(),
            error_reason: Some("Missing pqc_ct".to_string()),
        };
    }

    ParsedKeyDistContent {
        valid: true,
        group_id: parsed.group_id,
        // `sharedKey` from the event (if any) is deliberately dropped here.
        shared_pubkey: parsed.shared_pubkey,
        pqc_ct: parsed.pqc_ct,
        error_reason: None,
    }
}
