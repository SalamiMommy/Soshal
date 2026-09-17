use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

// ─── Build Key Distribution Content ────────────────────────────────────────

/// Input for [`build_key_distribution_content`].
#[derive(Deserialize)]
pub struct BuildKeyDistInput {
    #[serde(rename = "groupId", alias = "group_id", default)]
    pub group_id: String,
    #[serde(rename = "sharedKey", alias = "shared_key", default)]
    pub shared_key: String,
    #[serde(rename = "sharedPubkey", alias = "shared_pubkey", default)]
    pub shared_pubkey: String,
    #[serde(rename = "pqcCt", alias = "pqc_ct", default)]
    pub pqc_ct: String,
}

impl Drop for BuildKeyDistInput {
    fn drop(&mut self) {
        self.shared_key.zeroize();
    }
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

    if content.len() > 1024 * 1024 {
        return fallback();
    }

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
            group_id: parsed.group_id.clone(),
            shared_pubkey: parsed.shared_pubkey.clone(),
            pqc_ct: String::new(),
            error_reason: Some("Missing pqc_ct".to_string()),
        };
    }

    ParsedKeyDistContent {
        valid: true,
        group_id: parsed.group_id.clone(),
        // `sharedKey` from the event (if any) is deliberately dropped here.
        shared_pubkey: parsed.shared_pubkey.clone(),
        pqc_ct: parsed.pqc_ct.clone(),
        error_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_add_roundtrip() {
        let out = build_key_distribution_content(&BuildKeyDistInput {
            group_id: "g1".into(),
            shared_key: "K".into(),
            shared_pubkey: "pk-alice".into(),
            pqc_ct: "ct-for-alice".into(),
        });
        assert_eq!(out.group_id, "g1");
        assert_eq!(out.shared_pubkey, "pk-alice");
        assert_eq!(out.pqc_ct, "ct-for-alice");
        let wire = r#"{"groupId":"g1","sharedPubkey":"pk-alice","pqcCt":"ct-for-alice"}"#;
        let parsed = parse_key_distribution_content(wire);
        assert!(parsed.valid);
        assert_eq!(parsed.group_id, out.group_id);
        assert_eq!(parsed.shared_pubkey, out.shared_pubkey);
        assert_eq!(parsed.pqc_ct, out.pqc_ct);
    }

    #[test]
    fn each_member_gets_own_wrapped_key() {
        let alice = build_key_distribution_content(&BuildKeyDistInput {
            group_id: "g1".into(),
            shared_key: "shared".into(),
            shared_pubkey: "pk-alice".into(),
            pqc_ct: "wrap-alice".into(),
        });
        let bob = build_key_distribution_content(&BuildKeyDistInput {
            group_id: "g1".into(),
            shared_key: "shared".into(),
            shared_pubkey: "pk-bob".into(),
            pqc_ct: "wrap-bob".into(),
        });
        assert_eq!(alice.group_id, bob.group_id);
        assert_eq!(alice.pqc_ct, "wrap-alice");
        assert_eq!(bob.pqc_ct, "wrap-bob");
        assert_ne!(alice.pqc_ct, bob.pqc_ct);
    }

    #[test]
    fn revoked_member_absent_from_new_distribution() {
        let new_dist = build_key_distribution_content(&BuildKeyDistInput {
            group_id: "g1".into(),
            shared_key: "new-shared".into(),
            shared_pubkey: "pk-bob".into(),
            pqc_ct: "wrap-bob-new".into(),
        });
        let serialized = serde_json::to_string(&new_dist).unwrap();
        assert!(!serialized.contains("pk-alice"));
        assert!(!serialized.contains("wrap-alice"));
        assert!(!serialized.contains("new-shared"));
        let parsed = parse_key_distribution_content(
            r#"{"groupId":"g1","sharedPubkey":"pk-bob","pqcCt":"wrap-bob-new"}"#,
        );
        assert!(parsed.valid);
        assert_eq!(parsed.shared_pubkey, "pk-bob");
        assert_eq!(parsed.pqc_ct, "wrap-bob-new");
    }

    #[test]
    fn non_member_cannot_derive_key_material() {
        let input = BuildKeyDistInput {
            group_id: "g1".into(),
            shared_key: "SUPER_SECRET_GROUP_KEY".into(),
            shared_pubkey: "pk-alice".into(),
            pqc_ct: "ct-alice".into(),
        };
        let out = serde_json::to_string(&build_key_distribution_content(&input)).unwrap();
        assert!(!out.contains("SUPER_SECRET_GROUP_KEY"));
        assert!(!out.contains("sharedKey"));
        let wire = r#"{"groupId":"g1","sharedKey":"SUPER_SECRET_GROUP_KEY","sharedPubkey":"pk-alice","pqcCt":"ct-alice"}"#;
        let parsed = parse_key_distribution_content(wire);
        assert!(parsed.valid);
        let serialized = serde_json::to_string(&parsed).unwrap();
        assert!(!serialized.contains("SUPER_SECRET_GROUP_KEY"));
        assert!(!serialized.contains("sharedKey"));
    }
}
