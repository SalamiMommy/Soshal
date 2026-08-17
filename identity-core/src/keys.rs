use nostr::event::{SignEvent, UnsignedEvent};
use nostr::key::{Keys, PublicKey};
use nostr::nips::nip19::{FromBech32, ToBech32};

pub fn generate_keypair() -> Keys {
    soshal_nostr_core::keys::generate_keys()
}

pub fn parse_key(secret: &str) -> Result<Keys, String> {
    soshal_nostr_core::keys::from_nsec(secret).map_err(|e| format!("invalid key: {}", e))
}

pub fn public_key_hex(keys: &Keys) -> String {
    keys.public_key().to_string()
}

pub fn npub(keys: &Keys) -> String {
    keys.public_key().to_bech32().unwrap_or_default()
}

/// Encodes a hex x-only public key string into its bech32 npub form.
pub fn npub_encode(pubkey_hex: &str) -> Result<String, String> {
    let pk = PublicKey::from_hex(pubkey_hex).map_err(|e| format!("invalid pubkey: {}", e))?;
    pk.to_bech32().map_err(|e| format!("npub encode: {}", e))
}

/// Decodes an npub (bech32) into its hex x-only public key.
pub fn pubkey_from_npub(npub_str: &str) -> Result<String, String> {
    let pk = PublicKey::from_bech32(npub_str).map_err(|e| format!("invalid npub: {}", e))?;
    Ok(pk.to_string())
}

pub fn nsec(keys: &Keys) -> String {
    keys.secret_key().to_bech32().unwrap_or_default()
}

pub fn sign_event_json(keys: &Keys, event_json: &str) -> Result<String, String> {
    // Fast path: deserialize straight to UnsignedEvent (no serde_json::Value
    // round trip). Fall back to the tolerant Value path when the payload
    // carries a null id or the all-zeros placeholder id.
    let unsigned: UnsignedEvent = match serde_json::from_str(event_json) {
        Ok(u) => u,
        Err(_) => {
            let mut val: serde_json::Value = serde_json::from_str(event_json)
                .map_err(|e| format!("parse unsigned event: {}", e))?;
            if let Some(obj) = val.as_object_mut() {
                if obj.get("id").and_then(|v| v.as_str()) == Some(&"00".repeat(32))
                    || obj.get("id").map(|v| v.is_null()).unwrap_or(false)
                {
                    obj.remove("id");
                }
                obj.remove("sig");
            }
            serde_json::from_value(val).map_err(|e| format!("parse unsigned event: {}", e))?
        }
    };
    let signed = if unsigned
        .id
        .as_ref()
        .is_none_or(|id| id.as_bytes() == &[0u8; 32])
    {
        // Missing or placeholder id: rebuild without it so the id is derived
        // at sign time.
        let mut val: serde_json::Value =
            serde_json::from_str(event_json).map_err(|e| format!("parse unsigned event: {}", e))?;
        if let Some(obj) = val.as_object_mut() {
            obj.remove("id");
            obj.remove("sig");
        }
        let unsigned: UnsignedEvent =
            serde_json::from_value(val).map_err(|e| format!("parse unsigned event: {}", e))?;
        keys.sign_event(unsigned)
            .map_err(|e| format!("signing error: {}", e))?
    } else {
        keys.sign_event(unsigned)
            .map_err(|e| format!("signing error: {}", e))?
    };
    serde_json::to_string(&signed).map_err(|e| format!("serialize: {}", e))
}
