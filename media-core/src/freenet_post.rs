//! Freenet-native post contract state packaging and key resolution.

use serde::{Deserialize, Serialize};
use soshal_crypto_core::hash::sha256_hex;

/// Contract state representation of a post stored on the Freenet P2P network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FreenetPostPayload {
    pub author_pubkey: String,
    pub content: String,
    pub created_at: i64,
    pub reply_to: Option<String>,
    pub root_id: Option<String>,
    pub signature: Option<String>,
}

/// Result of building a Freenet post contract payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreenetPostContract {
    pub contract_key: String,
    pub payload_json: String,
    pub content_hash: String,
}

/// Builds a Freenet post contract from post parameters.
pub fn build_freenet_post_contract(
    author_pubkey: &str,
    content: &str,
    created_at: i64,
    reply_to: Option<String>,
    root_id: Option<String>,
    signature: Option<String>,
) -> Result<FreenetPostContract, String> {
    if content.is_empty() {
        return Err("content cannot be empty".into());
    }
    let payload = FreenetPostPayload {
        author_pubkey: author_pubkey.to_string(),
        content: content.to_string(),
        created_at,
        reply_to,
        root_id,
        signature,
    };
    let payload_json =
        serde_json::to_string(&payload).map_err(|e| format!("serialization error: {}", e))?;

    let content_hash = sha256_hex(payload_json.as_bytes());
    let raw_key = format!("freenet:post:{}:{}", author_pubkey, content_hash);
    let contract_key = format!("freenet://{}", sha256_hex(raw_key.as_bytes()));

    Ok(FreenetPostContract {
        contract_key,
        payload_json,
        content_hash,
    })
}

/// Verifies and unpacks a raw Freenet post contract payload JSON string.
pub fn verify_and_unpack_freenet_post(payload_json: &str) -> Result<FreenetPostPayload, String> {
    if payload_json.is_empty() || payload_json.len() > 1_048_576 {
        return Err("invalid payload length".into());
    }
    let payload: FreenetPostPayload =
        serde_json::from_str(payload_json).map_err(|e| format!("deserialization error: {}", e))?;
    if payload.author_pubkey.is_empty() {
        return Err("missing author pubkey".into());
    }
    Ok(payload)
}
