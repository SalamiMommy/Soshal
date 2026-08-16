//! Shared JSON envelope helpers for storage-core serializers.

/// Serializes a `{success:false, <field>:null, error}` failure envelope.
pub fn fail_json(field: &str, error: &str) -> String {
    serde_json::json!({
        "success": false,
        field: serde_json::Value::Null,
        "error": error,
    })
    .to_string()
}

/// Decodes a hex shared secret and validates it is exactly 32 bytes.
pub fn hex_to_32_bytes(ss_hex: &str) -> Result<[u8; 32], String> {
    let ss = hex::decode(ss_hex).map_err(|e| e.to_string())?;
    if ss.len() != 32 {
        return Err("bad ss len".to_string());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&ss);
    Ok(arr)
}
