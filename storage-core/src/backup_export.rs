use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::json_out;

/// Table names exported per-user, in export order.
pub const EXPORT_TABLES: [&str; 6] = [
    "users",
    "posts",
    "reactions",
    "zaps",
    "notifications",
    "bookmarks",
];

/// Converts a libsql row into a JSON object keyed by column name. Blobs are
/// base64-encoded for wire safety.
pub fn row_to_json(row: &libsql::Row, cols: &[String]) -> serde_json::Value {
    let mut obj = serde_json::Map::with_capacity(cols.len());
    for (idx, name) in cols.iter().enumerate() {
        let val: serde_json::Value = match row.get_value(idx as i32) {
            Ok(libsql::Value::Null) => serde_json::Value::Null,
            Ok(libsql::Value::Integer(n)) => serde_json::json!(n),
            Ok(libsql::Value::Real(f)) => serde_json::json!(f),
            Ok(libsql::Value::Text(t)) => serde_json::json!(t),
            Ok(libsql::Value::Blob(b)) => {
                serde_json::json!(soshal_crypto_core::base64::base64_encode_bytes(&b))
            }
            Err(_) => serde_json::Value::Null,
        };
        obj.insert(name.clone(), val);
    }
    serde_json::Value::Object(obj)
}

/// Builds the export envelope `{exportedAt, version, data}` from per-table
/// row arrays. Versions the payload so future restores can detect format
/// drift.
pub fn export_envelope(
    now_ms: u64,
    tables: &[(&str, Vec<serde_json::Value>)],
) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    for (name, rows) in tables {
        data.insert((*name).to_string(), serde_json::Value::Array(rows.clone()));
    }
    serde_json::json!({
        "exportedAt": now_ms,
        "version": "1.0.0",
        "data": serde_json::Value::Object(data),
    })
}

#[derive(Deserialize)]
struct BackupExportInput {
    db_json: String,
    kem_public_key_hex: String,
}

#[derive(Serialize)]
struct BackupExportOutput {
    success: bool,
    ciphertext_json: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct EncryptedBackupEnvelope {
    pqc_ct: String,
    nonce: String,
    payload: String,
    version: u32,
}

pub fn create_encrypted_backup_payload(input: &str) -> String {
    let input: BackupExportInput = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => return crate::util::fail_json("ciphertext_json", &format!("JSON parse: {}", e)),
    };

    let compressed = match soshal_content_core::compress::compress(input.db_json.as_bytes()) {
        Ok(c) => c,
        Err(e) => return crate::util::fail_json("ciphertext_json", &format!("compress: {}", e)),
    };

    let domain = b"soshal-backup-export-v1";
    let (ct_hex, nonce_hex, encrypted_b64) =
        match soshal_pqc_core::seal::hybrid_seal(&compressed, &input.kem_public_key_hex, domain) {
            Ok(v) => v,
            Err(e) => return crate::util::fail_json("ciphertext_json", &format!("seal: {}", e)),
        };

    let envelope = EncryptedBackupEnvelope {
        pqc_ct: ct_hex,
        nonce: nonce_hex,
        payload: encrypted_b64,
        version: 3,
    };

    let out = BackupExportOutput {
        success: true,
        ciphertext_json: serde_json::to_string(&envelope).ok(),
        error: None,
    };

    json_out(&out, r#"{"success":false,"error":"serialization failed"}"#)
}
