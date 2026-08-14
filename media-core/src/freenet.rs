use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::{json_in, json_out};

#[doc(hidden)]
pub fn freenet_contract_hash(key: &str, parameters: &str) -> String {
    let mut combined = Vec::with_capacity(key.len() + 1 + parameters.len());
    combined.extend_from_slice(key.as_bytes());
    combined.push(0x00);
    combined.extend_from_slice(parameters.as_bytes());
    let hash = soshal_crypto_core::hash::sha256(&combined);
    hex::encode(hash)
}

#[doc(hidden)]
pub fn freenet_content_hash(data_b64: &str) -> String {
    let decoded = match soshal_crypto_core::base64::base64_decode_bytes(data_b64) {
        Some(bytes) => bytes,
        None => return String::new(),
    };
    let hash = soshal_crypto_core::hash::sha256(&decoded);
    hex::encode(hash)
}

#[derive(Deserialize)]
struct ContractHashInput {
    key: String,
    #[serde(default)]
    parameters: String,
}

#[derive(Serialize)]
struct ContractHashOutput {
    contract_hash: String,
}

#[derive(Deserialize)]
struct ContentHashInput {
    data: String,
}

#[derive(Serialize)]
struct ContentHashOutput {
    content_hash: String,
}

pub fn freenet_contract_hash_json(input: &str) -> String {
    if input.len() > 262_144 {
        return String::new();
    }
    let Some(input) = json_in::<Option<ContractHashInput>>(input, None) else {
        return String::new();
    };
    let hash = freenet_contract_hash(&input.key, &input.parameters);
    let output = ContractHashOutput {
        contract_hash: hash,
    };
    json_out(&output, "")
}

pub fn freenet_content_hash_json(input: &str) -> String {
    if input.len() > 262_144 {
        return String::new();
    }
    let Some(input) = json_in::<Option<ContentHashInput>>(input, None) else {
        return String::new();
    };
    let hash = freenet_content_hash(&input.data);
    let output = ContentHashOutput { content_hash: hash };
    json_out(&output, "")
}
