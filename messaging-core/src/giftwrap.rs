use serde::{Deserialize, Serialize};

use soshal_common_core::json_util::{json_in_borrow, json_out};

use std::borrow::Cow;

#[derive(Deserialize)]
struct BuildRumorEnvelopeInput<'a> {
    #[serde(rename = "pqcCt")]
    pqc_ct: Cow<'a, str>,
    rumor: Cow<'a, str>,
}

#[derive(Serialize)]
struct RumorEnvelopeOutput<'a> {
    #[serde(rename = "pqc_ct")]
    pqc_ct: &'a str,
    rumor: &'a str,
}

pub fn build_rumor_envelope_json(input: &str) -> String {
    let Some(input) = json_in_borrow::<BuildRumorEnvelopeInput>(input) else {
        return String::new();
    };
    let out = RumorEnvelopeOutput {
        pqc_ct: &input.pqc_ct,
        rumor: &input.rumor,
    };
    json_out(&out, "")
}

#[derive(Deserialize)]
struct BuildSealEnvelopeInput<'a> {
    #[serde(rename = "pqcCt")]
    pqc_ct: Cow<'a, str>,
    #[serde(rename = "rumorJson")]
    rumor_json: Cow<'a, str>,
    #[serde(rename = "dsaPublicKey")]
    dsa_public_key: Cow<'a, str>,
    #[serde(rename = "peerDsaPublicKey")]
    peer_dsa_public_key: Option<Cow<'a, str>>,
}

#[derive(Serialize)]
struct SealEnvelopeOutput<'a> {
    #[serde(rename = "pqc_ct")]
    pqc_ct: &'a str,
    rumor: &'a str,
    #[serde(rename = "pqc_pk")]
    pqc_pk: &'a str,
    #[serde(rename = "pqc_pk_peer", skip_serializing_if = "Option::is_none")]
    pqc_pk_peer: Option<&'a str>,
}

pub fn build_seal_envelope_json(input: &str) -> String {
    let Some(input) = json_in_borrow::<BuildSealEnvelopeInput>(input) else {
        return String::new();
    };
    let out = SealEnvelopeOutput {
        pqc_ct: &input.pqc_ct,
        rumor: &input.rumor_json,
        pqc_pk: &input.dsa_public_key,
        pqc_pk_peer: input.peer_dsa_public_key.as_deref(),
    };
    json_out(&out, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn rumor_with_sender(sender: &str) -> String {
        format!(r#"{{"content":"hi","sender":"{sender}"}}"#)
    }

    #[test]
    fn rumor_envelope_roundtrip() {
        let input = r#"{"pqcCt":"abcd1234","rumor":"{\"content\":\"hi\"}"}"#;
        let out = build_rumor_envelope_json(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["pqc_ct"], "abcd1234");
        assert_eq!(v["rumor"], r#"{"content":"hi"}"#);
    }

    #[test]
    fn tampered_input_fails_empty() {
        assert_eq!(build_rumor_envelope_json("not json"), "");
        assert_eq!(build_rumor_envelope_json(r#"{"pqcCt":1,"rumor":"x"}"#), "");
        assert_eq!(build_seal_envelope_json("garbage"), "");
    }

    #[test]
    fn tampered_rumor_detected_in_output() {
        let input = r#"{"pqcCt":"abcd","rumor":"{\"content\":\"hi\"}"}"#;
        let tampered = r#"{"pqcCt":"abcd","rumor":"{\"content\":\"evil\"}"}"#;
        let v1: Value = serde_json::from_str(&build_rumor_envelope_json(input)).unwrap();
        let v2: Value = serde_json::from_str(&build_rumor_envelope_json(tampered)).unwrap();
        assert_ne!(v1["rumor"], v2["rumor"]);
    }

    #[test]
    fn sender_recovered_from_seal_rumor() {
        let rumor = rumor_with_sender("npub_sender");
        let rumor_escaped = serde_json::to_string(&rumor).unwrap();
        let input = format!(
            r#"{{"pqcCt":"ct","rumorJson":{rumor_escaped},"dsaPublicKey":"pk","peerDsaPublicKey":"peer"}}"#
        );
        let out = build_seal_envelope_json(&input);
        let v: Value = serde_json::from_str(&out).unwrap();
        let rumor_out: Value = serde_json::from_str(v["rumor"].as_str().unwrap()).unwrap();
        assert_eq!(rumor_out["sender"], "npub_sender");
    }

    #[test]
    fn seal_recipient_key_passthrough() {
        let input =
            r#"{"pqcCt":"ct","rumorJson":"{}","dsaPublicKey":"pk1","peerDsaPublicKey":"pk2"}"#;
        let out = build_seal_envelope_json(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["pqc_pk"], "pk1");
        assert_eq!(v["pqc_pk_peer"], "pk2");
    }

    #[test]
    fn seal_omits_recipient_key_when_absent() {
        let input = r#"{"pqcCt":"ct","rumorJson":"{}","dsaPublicKey":"pk1"}"#;
        let out = build_seal_envelope_json(input);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("pqc_pk_peer").is_none());
    }
}
