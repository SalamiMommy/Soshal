use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::json_out;
use soshal_crypto_core::hash;

/// Reject envelopes whose signature timestamp is older than this delta in
/// either direction (replay protection + clock-skew bound).
const MAX_TS_SKEW_SECS: i64 = 7 * 24 * 3600;

#[derive(Serialize, Deserialize)]
pub struct OfflineSyncEnvelope {
    pub pqc_ct: String,
    pub inner: String,
    pub sender_pubkey: Option<String>,
    pub sender_dsa_pubkey: Option<String>,
    pub dsa_sig: Option<String>,
    #[serde(default)]
    pub ts: Option<i64>,
}

#[derive(Deserialize)]
pub struct DecryptOfflineSyncInput {
    pub envelope_json: String,
    pub context: String,
    pub kem_secret_key_hex: String,
    pub dsa_public_key_hex: String,
    /// The pubkey the recipient expects this envelope to come from. Must
    /// match the sender the envelope claims; mismatches are rejected.
    pub sender_pubkey: String,
}

#[derive(Serialize)]
pub struct DecryptOfflineSyncOutput {
    pub success: bool,
    pub payload_json: Option<String>,
    pub error: Option<String>,
}

fn derive_conversation_key(shared_secret: &[u8; 32], context: &str) -> Option<[u8; 32]> {
    let okm = hash::hkdf_sha256(
        shared_secret,
        b"soshal-conversation-key-salt-v1",
        context.as_bytes(),
        32,
    );
    let mut key = [0u8; 32];
    key.copy_from_slice(&okm.ok()?);
    Some(key)
}

fn fail_decrypt(error: &str) -> String {
    crate::util::fail_json("payload_json", error)
}

pub fn decrypt_offline_sync_json(input: &str) -> String {
    let input: DecryptOfflineSyncInput = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => return fail_decrypt(&format!("JSON parse: {}", e)),
    };

    let env: OfflineSyncEnvelope = match serde_json::from_str(&input.envelope_json) {
        Ok(e) => e,
        Err(e) => return fail_decrypt(&format!("envelope parse: {}", e)),
    };

    let dsa_sig = match env.dsa_sig.as_ref() {
        Some(sig) => sig,
        None => return fail_decrypt("missing DSA signature"),
    };
    if input.dsa_public_key_hex.is_empty() {
        return fail_decrypt("missing DSA verifier key");
    }
    // SECURITY: the claimed sender must match the sender the caller expects,
    // and the DSA public key used for verification must be the one the sender
    // announced inside the envelope. Together with the timestamp this stops
    // replay against a different recipient and forged sender attribution.
    let Some(env_sender) = env.sender_pubkey.as_deref() else {
        return fail_decrypt("envelope has no sender_pubkey");
    };
    if env_sender != input.sender_pubkey {
        return fail_decrypt("envelope sender does not match expected sender");
    }
    if env.sender_dsa_pubkey.as_deref() != Some(input.dsa_public_key_hex.as_str()) {
        return fail_decrypt("envelope sender DSA key does not match verifier key");
    }
    let Some(ts) = env.ts else {
        return fail_decrypt("envelope has no timestamp");
    };
    let now = soshal_common_core::format::now_secs();
    if (now - ts).abs() > MAX_TS_SKEW_SECS {
        return fail_decrypt("envelope timestamp outside acceptable window");
    }
    let verify_msg = format!(
        "{}|{}|{}|{}|{}",
        env_sender,
        env.sender_dsa_pubkey.as_deref().unwrap_or(""),
        ts,
        env.pqc_ct,
        env.inner
    );
    if !soshal_pqc_core::dsa::dsa_verify_hex(
        dsa_sig,
        verify_msg.as_bytes(),
        &input.dsa_public_key_hex,
    ) {
        return fail_decrypt("DSA signature verification failed");
    }

    let ss_hex = match soshal_pqc_core::kem::kem_decapsulate(&env.pqc_ct, &input.kem_secret_key_hex)
    {
        Ok(ss) => ss,
        Err(e) => return fail_decrypt(&format!("KEM decap: {}", e)),
    };

    let ss_arr = match crate::util::hex_to_32_bytes(&ss_hex) {
        Ok(v) => v,
        Err(e) => return fail_decrypt(&e),
    };
    let conv_key = match derive_conversation_key(&ss_arr, &input.context) {
        Some(k) => k,
        None => return fail_decrypt("key derivation failed"),
    };

    let compressed_payload = match soshal_crypto_core::nip44::decrypt(&env.inner, &conv_key) {
        Ok(pt) => pt,
        Err(e) => return fail_decrypt(&format!("NIP-44 decrypt: {}", e)),
    };

    let decompressed = match soshal_content_core::compress::decompress(&compressed_payload) {
        Ok(d) => String::from_utf8(d).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(&compressed_payload).to_string(),
    };

    json_out(
        &DecryptOfflineSyncOutput {
            success: true,
            payload_json: Some(decompressed),
            error: None,
        },
        &fail_decrypt("serialization failed"),
    )
}
