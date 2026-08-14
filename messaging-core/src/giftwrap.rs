use serde::{Deserialize, Serialize};

use soshal_common_core::json_util::{json_in, json_out};

#[derive(Deserialize)]
struct BuildRumorEnvelopeInput {
    #[serde(rename = "pqcCt")]
    pqc_ct: String,
    rumor: String,
}

#[derive(Serialize)]
struct RumorEnvelopeOutput {
    #[serde(rename = "pqc_ct")]
    pqc_ct: String,
    rumor: String,
}

fn build_rumor_envelope(pqc_ct: String, rumor: String) -> RumorEnvelopeOutput {
    RumorEnvelopeOutput { pqc_ct, rumor }
}

pub fn build_rumor_envelope_json(input: &str) -> String {
    let Some(input) = json_in::<Option<BuildRumorEnvelopeInput>>(input, None) else {
        return String::new();
    };
    let out = build_rumor_envelope(input.pqc_ct, input.rumor);
    json_out(&out, "")
}

#[derive(Deserialize)]
struct BuildSealEnvelopeInput {
    #[serde(rename = "pqcCt")]
    pqc_ct: String,
    #[serde(rename = "rumorJson")]
    rumor_json: String,
    #[serde(rename = "dsaPublicKey")]
    dsa_public_key: String,
    #[serde(rename = "peerDsaPublicKey")]
    peer_dsa_public_key: Option<String>,
}

#[derive(Serialize)]
struct SealEnvelopeOutput {
    #[serde(rename = "pqc_ct")]
    pqc_ct: String,
    rumor: String,
    #[serde(rename = "pqc_pk")]
    pqc_pk: String,
    #[serde(rename = "pqc_pk_peer", skip_serializing_if = "Option::is_none")]
    pqc_pk_peer: Option<String>,
}

fn build_seal_envelope(input: BuildSealEnvelopeInput) -> SealEnvelopeOutput {
    SealEnvelopeOutput {
        pqc_ct: input.pqc_ct,
        rumor: input.rumor_json,
        pqc_pk: input.dsa_public_key,
        pqc_pk_peer: input.peer_dsa_public_key,
    }
}

pub fn build_seal_envelope_json(input: &str) -> String {
    let Some(input) = json_in::<Option<BuildSealEnvelopeInput>>(input, None) else {
        return String::new();
    };
    let out = build_seal_envelope(input);
    json_out(&out, "")
}
