//! F2F backup restore: decrypt a backup envelope + verify integrity.

use serde::{Deserialize, Serialize};
use soshal_common_core::json_util::json_out;

/// Per-post metadata carried alongside the event, mapped back onto the
/// posts table during restore.
#[derive(Clone, Default, Debug)]
pub struct BackupPostMeta {
    pub reply_to: Option<String>,
    pub root_id: Option<String>,
    pub mentioned_pubkeys: String,
    pub mentioned_hashtags: String,
    pub subject: Option<String>,
    pub is_deleted: bool,
    pub scheduled_at: Option<i64>,
}

/// Unwraps a `decrypt_backup_payload` result string into the decrypted
/// `db_json` value. Returns `Err` on envelope errors or decrypt failure.
pub fn unwrap_backup_db(out: &str) -> Result<serde_json::Value, String> {
    let parsed: serde_json::Value = serde_json::from_str(out).map_err(|e| e.to_string())?;
    if !parsed["success"].as_bool().unwrap_or(false) {
        return Err(parsed["error"]
            .as_str()
            .unwrap_or("restore failed")
            .to_string());
    }
    serde_json::from_str(parsed["db_json"].as_str().unwrap_or("{}")).map_err(|e| e.to_string())
}

/// Extracts the canonical event JSON from a backup post row, or `None` when
/// the row lacks an id or signature. Signature verification is the caller's
/// job (requires the nostr stack).
pub fn backup_post_event_payload(p: &serde_json::Value) -> Option<serde_json::Value> {
    let id = p.get("id").and_then(|v| v.as_str())?;
    if id.is_empty() {
        return None;
    }
    let sig = p.get("sig").and_then(|v| v.as_str())?;
    let tags_json = p.get("tags_json").and_then(|v| v.as_str()).unwrap_or("[]");
    Some(serde_json::json!({
        "id": id,
        "pubkey": p.get("pubkey").and_then(|v| v.as_str()).unwrap_or(""),
        "created_at": p.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0),
        "kind": p.get("kind").and_then(|v| v.as_i64()).unwrap_or(1),
        "tags": serde_json::from_str::<serde_json::Value>(tags_json)
            .unwrap_or(serde_json::Value::Array(vec![])),
        "content": p.get("content").and_then(|v| v.as_str()).unwrap_or(""),
        "sig": sig,
    }))
}

/// Extracts non-event metadata from a backup post row.
pub fn backup_post_meta(p: &serde_json::Value) -> BackupPostMeta {
    BackupPostMeta {
        reply_to: p
            .get("reply_to")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        root_id: p
            .get("root_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        mentioned_pubkeys: p
            .get("mentioned_pubkeys")
            .and_then(|v| v.as_str())
            .unwrap_or("[]")
            .to_string(),
        mentioned_hashtags: p
            .get("mentioned_hashtags")
            .and_then(|v| v.as_str())
            .unwrap_or("[]")
            .to_string(),
        subject: p
            .get("subject")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        is_deleted: p
            .get("is_deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        scheduled_at: p.get("scheduled_at").and_then(|v| v.as_i64()),
    }
}

#[derive(Deserialize)]
struct RestoreInput {
    payload: String,
    sk_hex: String,
}

#[derive(Serialize)]
struct RestoreOutput {
    success: bool,
    db_json: Option<String>,
    error: Option<String>,
}

/// Envelope format version written by `backup_export`.
const ENVELOPE_VERSION: u32 = 3;

#[derive(Deserialize)]
struct BackupEnvelope {
    pqc_ct: String,
    nonce: String,
    payload: String,
    version: u32,
}

/// Decrypts a sealed backup envelope produced by `backup_export` and returns the
/// original JSON blob. Domain must match `soshal-backup-export-v1`.
pub fn decrypt_backup_payload(input: &str) -> String {
    let input: RestoreInput = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(e) => return crate::util::fail_json("db_json", &format!("JSON parse: {}", e)),
    };

    let envelope: BackupEnvelope = match serde_json::from_str(&input.payload) {
        Ok(v) => v,
        Err(e) => return crate::util::fail_json("db_json", &format!("envelope parse: {}", e)),
    };
    if envelope.version != ENVELOPE_VERSION {
        return crate::util::fail_json(
            "db_json",
            &format!(
                "unsupported envelope version {} (expected {})",
                envelope.version, ENVELOPE_VERSION
            ),
        );
    }

    let domain = b"soshal-backup-export-v1";
    let compressed = match soshal_pqc_core::seal::hybrid_unseal(
        &envelope.payload,
        &envelope.nonce,
        &envelope.pqc_ct,
        &input.sk_hex,
        domain,
    ) {
        Ok(v) => v,
        Err(e) => return crate::util::fail_json("db_json", &format!("unseal: {}", e)),
    };

    let plaintext = match soshal_content_core::compress::decompress(&compressed) {
        Ok(v) => v,
        Err(e) => return crate::util::fail_json("db_json", &format!("decompress: {}", e)),
    };

    let db_json = match String::from_utf8(plaintext) {
        Ok(s) => s,
        Err(_) => return crate::util::fail_json("db_json", "decrypted payload is not UTF-8"),
    };

    // Verify the inner JSON is valid before handing it to the importer.
    if serde_json::from_str::<serde_json::Value>(&db_json).is_err() {
        return crate::util::fail_json("db_json", "decrypted payload is not valid JSON");
    }

    let out = RestoreOutput {
        success: true,
        db_json: Some(db_json),
        error: None,
    };

    json_out(&out, r#"{"success":false,"error":"serialization failed"}"#)
}
