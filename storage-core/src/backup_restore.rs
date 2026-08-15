//! F2F backup restore helpers.

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

/// Unwraps a backup decrypt result string into the decrypted
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup_export::export_envelope;

    fn wrap_success(env: &serde_json::Value) -> String {
        serde_json::json!({
            "success": true,
            "db_json": serde_json::to_string(env).unwrap(),
        })
        .to_string()
    }

    fn sample_post_row() -> serde_json::Value {
        serde_json::json!({
            "id": "e1", "pubkey": "pk1", "created_at": 100, "kind": 1,
            "tags_json": r#"[["t","x"]]"#, "content": "hello", "sig": "sig1",
            "reply_to": "r1", "root_id": "root", "mentioned_pubkeys": "[\"a\"]",
            "mentioned_hashtags": "[\"h\"]", "subject": "sub", "is_deleted": true,
            "scheduled_at": 42
        })
    }

    #[test]
    fn test_restore_roundtrip_yields_original_data() {
        let env = export_envelope(99, &[("posts", vec![sample_post_row()])]);
        let restored = unwrap_backup_db(&wrap_success(&env)).unwrap();
        assert_eq!(restored["exportedAt"], 99u64);
        assert_eq!(restored["version"], "1.0.0");
        let post = &restored["data"]["posts"][0];
        let ev = backup_post_event_payload(post).unwrap();
        assert_eq!(ev["id"], "e1");
        assert_eq!(ev["pubkey"], "pk1");
        assert_eq!(ev["created_at"], 100);
        assert_eq!(ev["kind"], 1);
        assert_eq!(ev["content"], "hello");
        assert_eq!(ev["sig"], "sig1");
        assert_eq!(ev["tags"][0][0], "t");
        let meta = backup_post_meta(post);
        assert_eq!(meta.reply_to.as_deref(), Some("r1"));
        assert_eq!(meta.root_id.as_deref(), Some("root"));
        assert_eq!(meta.subject.as_deref(), Some("sub"));
        assert!(meta.is_deleted);
        assert_eq!(meta.scheduled_at, Some(42));
        assert_eq!(meta.mentioned_pubkeys, "[\"a\"]");
        assert_eq!(meta.mentioned_hashtags, "[\"h\"]");
    }

    #[test]
    fn test_unwrap_backup_db_rejects_flipped_bytes() {
        let valid = wrap_success(&export_envelope(1, &[]));
        assert!(unwrap_backup_db(&valid).is_ok());
        let mut bytes = valid.into_bytes();
        bytes[1] ^= 0xFF;
        bytes[2] ^= 0xFF;
        assert!(unwrap_backup_db(&String::from_utf8_lossy(&bytes)).is_err());
    }

    #[test]
    fn test_unwrap_backup_db_wrong_format_errors() {
        assert_eq!(unwrap_backup_db("{}").unwrap_err(), "restore failed");
        assert!(unwrap_backup_db(r#"{"success":"yes"}"#).is_err());
        assert!(unwrap_backup_db("garbage").is_err());
        assert!(unwrap_backup_db("").is_err());
        assert!(unwrap_backup_db(r#"{"success":false,"error":"boom"}"#)
            .unwrap_err()
            .eq("boom"));
    }

    #[test]
    fn test_unwrap_backup_db_atomic_no_partial_result() {
        let valid = wrap_success(&export_envelope(1, &[]));
        let truncated = valid[..valid.len() / 2].to_string();
        assert!(unwrap_backup_db(&truncated).is_err());
        let bad_db = r#"{"success":true,"db_json":"{invalid"}"#;
        assert!(unwrap_backup_db(bad_db).is_err());
    }
}
