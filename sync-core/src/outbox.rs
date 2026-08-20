//! Outbox queue processor and offline background upload engine.

use std::io::Read;

use libsql::params;
use serde::{Deserialize, Serialize};
use soshal_db_core::{block_on, query::query_first, Database};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub action_type: String,
    pub payload_json: String,
    pub media_path: Option<String>,
    pub status: String,
    pub retry_count: i32,
    pub next_retry_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxSummary {
    pub pending_count: u32,
    pub failed_count: u32,
    pub total_count: u32,
}

const ZSTD_MAGIC: &[u8; 4] = b"Zstd";

pub fn compress_payload(payload: &str) -> String {
    if payload.len() > 256 {
        if let Ok(compressed) = zstd::encode_all(payload.as_bytes(), 3) {
            let mut buf = Vec::with_capacity(4 + compressed.len());
            buf.extend_from_slice(ZSTD_MAGIC);
            buf.extend_from_slice(&compressed);
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
            return format!("__zstd_b64__:{b64}");
        }
    }
    payload.to_string()
}

pub fn decompress_payload(raw: &str) -> String {
    const MAX_DECOMPRESSED: u64 = 1024 * 1024;
    let cap = |bytes: &[u8]| -> Option<String> {
        let decoder = zstd::stream::read::Decoder::new(bytes).ok()?;
        let mut out = Vec::new();
        decoder
            .take(MAX_DECOMPRESSED + 1)
            .read_to_end(&mut out)
            .ok()?;
        if out.len() > MAX_DECOMPRESSED as usize {
            return None;
        }
        String::from_utf8(out).ok()
    };
    if let Some(b64_data) = raw.strip_prefix("__zstd_b64__:") {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64_data) {
            if bytes.len() > 4 && &bytes[..4] == ZSTD_MAGIC {
                if let Some(s) = cap(&bytes[4..]) {
                    return s;
                }
            }
        }
    } else if let Some(hex_data) = raw.strip_prefix("__zstd__:") {
        if let Ok(bytes) = hex::decode(hex_data) {
            if bytes.len() > 4 && &bytes[..4] == ZSTD_MAGIC {
                if let Some(s) = cap(&bytes[4..]) {
                    return s;
                }
            }
        }
    }
    raw.to_string()
}

pub fn enqueue_outbox_item(
    db: &Database,
    id: &str,
    action_type: &str,
    payload_json: &str,
    media_path: Option<&str>,
    now_secs: i64,
) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let stored_payload = compress_payload(payload_json);
    block_on(conn.execute(
        "INSERT INTO outbox_queue (id, action_type, payload_json, media_path, status, retry_count, next_retry_at, created_at)
         VALUES (?1, ?2, ?3, ?4, 'pending', 0, 0, ?5)",
        params![id, action_type, stored_payload, media_path, now_secs],
    ))
    .map_err(|e| format!("Failed to enqueue outbox item: {e}"))?;
    Ok(())
}

pub fn fetch_pending_outbox_items(
    db: &Database,
    now_secs: i64,
    limit: usize,
) -> Result<Vec<OutboxItem>, String> {
    fetch_pending_outbox_items_filtered(db, now_secs, limit, false)
}

pub fn fetch_pending_outbox_items_filtered(
    db: &Database,
    now_secs: i64,
    limit: usize,
    include_media: bool,
) -> Result<Vec<OutboxItem>, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(async {
        let sql = if include_media {
            "SELECT id, action_type, payload_json, media_path, status, retry_count, next_retry_at, created_at
             FROM outbox_queue
             WHERE status = 'pending' AND next_retry_at <= ?1
             ORDER BY created_at ASC
             LIMIT ?2"
        } else {
            "SELECT id, action_type, payload_json, media_path, status, retry_count, next_retry_at, created_at
             FROM outbox_queue
             WHERE status = 'pending' AND media_path IS NULL AND next_retry_at <= ?1
             ORDER BY created_at ASC
             LIMIT ?2"
        };
        let mut stmt = conn.prepare(sql).await.map_err(|e| e.to_string())?;

        let mut rows = stmt
            .query(params![now_secs, limit as i64])
            .await
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let raw_payload: String = row.get(2).map_err(|e| e.to_string())?;
            out.push(OutboxItem {
                id: row.get(0).map_err(|e| e.to_string())?,
                action_type: row.get(1).map_err(|e| e.to_string())?,
                payload_json: decompress_payload(&raw_payload),
                media_path: row.get(3).map_err(|e| e.to_string())?,
                status: row.get(4).map_err(|e| e.to_string())?,
                retry_count: row.get(5).map_err(|e| e.to_string())?,
                next_retry_at: row.get(6).map_err(|e| e.to_string())?,
                created_at: row.get(7).map_err(|e| e.to_string())?,
            });
        }
        Ok(out)
    })
}

pub fn mark_outbox_item_completed(db: &Database, id: &str) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(conn.execute(
        "UPDATE outbox_queue SET status = 'completed' WHERE id = ?1",
        params![id],
    ))
    .map_err(|e| format!("Failed to update outbox item status: {e}"))?;
    Ok(())
}

/// Record a failed outbox attempt. `retry_count` is the already-incremented
/// attempt number; `next_retry_at` is the caller-computed backoff deadline
/// (see `engine::retry_at`). Status flips to `failed` at 10 attempts.
pub fn mark_outbox_item_failed(
    db: &Database,
    id: &str,
    retry_count: i32,
    next_retry_at: i64,
) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let status = if retry_count >= 10 {
        "failed"
    } else {
        "pending"
    };
    block_on(conn.execute(
        "UPDATE outbox_queue SET status = ?1, retry_count = ?2, next_retry_at = ?3 WHERE id = ?4",
        params![status, retry_count, next_retry_at, id],
    ))
    .map_err(|e| format!("Failed to update outbox item failure: {e}"))?;
    Ok(())
}

pub fn summarize_outbox(db: &Database) -> Result<OutboxSummary, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let pending: i64 = query_first(
        &conn,
        "SELECT COUNT(*) FROM outbox_queue WHERE status = 'pending'",
        (),
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())?
    .unwrap_or(0);
    let failed: i64 = query_first(
        &conn,
        "SELECT COUNT(*) FROM outbox_queue WHERE status = 'failed'",
        (),
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())?
    .unwrap_or(0);
    let total: i64 = query_first(&conn, "SELECT COUNT(*) FROM outbox_queue", (), |r| r.get(0))
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    Ok(OutboxSummary {
        pending_count: pending.max(0) as u32,
        failed_count: failed.max(0) as u32,
        total_count: total as u32,
    })
}

/// Mark many outbox items completed in a single transaction.
pub fn mark_outbox_items_completed(db: &Database, ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let conn = db.conn().map_err(|e| e.to_string())?;
    let ids_json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    block_on(conn.execute(
        "UPDATE outbox_queue SET status = 'completed' WHERE id IN (SELECT value FROM json_each(?1))",
        params![ids_json],
    ))
    .map_err(|e| format!("batch outbox complete: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outbox_workflow() {
        let db = soshal_test_util::test_db();

        enqueue_outbox_item(&db, "item1", "post", "{}", None, 100).unwrap();
        let pending = fetch_pending_outbox_items(&db, 100, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "item1");

        mark_outbox_item_completed(&db, "item1").unwrap();
        let summary = summarize_outbox(&db).unwrap();
        assert_eq!(summary.pending_count, 0);
        assert_eq!(summary.total_count, 1);
    }

    #[test]
    fn test_outbox_fetch_ordering_limit_and_retry_gate() {
        let db = soshal_test_util::test_db();
        // Older items surface first (created_at ASC).
        enqueue_outbox_item(&db, "old", "post", "{}", None, 100).unwrap();
        enqueue_outbox_item(&db, "new", "post", "{}", None, 200).unwrap();
        let pending = fetch_pending_outbox_items(&db, 100, 10).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, "old");
        // next_retry_at in the future excludes the item.
        mark_outbox_item_failed(&db, "old", 0, 101).unwrap();
        let pending = fetch_pending_outbox_items(&db, 100, 10).unwrap();
        assert_eq!(pending.len(), 1, "retry-gated item excluded");
        assert_eq!(pending[0].id, "new");
        // Limit respected.
        let pending = fetch_pending_outbox_items(&db, 10_000, 1).unwrap();
        assert_eq!(pending.len(), 1);
        // Caller contract: passes the already-incremented retry count and a
        // computed next_retry_at (see engine::retry_at); status flips to
        // failed at attempt 10.
        mark_outbox_item_failed(&db, "old", 1, 10_000).unwrap();
        mark_outbox_item_failed(&db, "old", 9, 10_300 + 256).unwrap();
        let items = fetch_pending_outbox_items(&db, 10_600, 10).unwrap();
        let old = items.iter().find(|i| i.id == "old").unwrap();
        assert_eq!(old.retry_count, 9);
        assert_eq!(
            old.next_retry_at,
            10_300 + 256,
            "backoff capped at 256s (2^8) by design"
        );
        mark_outbox_item_failed(&db, "old", 10, 10_600).unwrap();
        let summary = summarize_outbox(&db).unwrap();
        assert_eq!(summary.failed_count, 1, "retry cap reached");
        // Batch completion flips the rest.
        mark_outbox_items_completed(&db, &["new".to_string()]).unwrap();
        let summary = summarize_outbox(&db).unwrap();
        assert_eq!(summary.pending_count, 0);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.total_count, 2);
        // Empty batch is a no-op.
        mark_outbox_items_completed(&db, &[]).unwrap();
    }

    #[test]
    fn test_payload_compress_roundtrip_and_garbage() {
        // Small payloads are stored verbatim.
        assert_eq!(compress_payload("short"), "short");
        assert_eq!(decompress_payload("not-compressed"), "not-compressed");
        // Large payloads roundtrip through zstd (Base64).
        let big = "x".repeat(10_000);
        let stored = compress_payload(&big);
        assert!(
            stored.starts_with("__zstd_b64__:"),
            "stored: {}..",
            &stored[..24]
        );
        assert_eq!(decompress_payload(&stored), big);
        // Legacy hex format backward compatibility.
        let mut buf = Vec::new();
        buf.extend_from_slice(ZSTD_MAGIC);
        buf.extend_from_slice(&zstd::encode_all(big.as_bytes(), 3).unwrap());
        let legacy_valid = format!("__zstd__:{}", hex::encode(buf));
        assert_eq!(decompress_payload(&legacy_valid), big);
        // Garbage prefixed payloads degrade to the raw string.
        assert_eq!(decompress_payload("__zstd_b64__:zz"), "__zstd_b64__:zz");
        assert_eq!(decompress_payload("__zstd__:zz"), "__zstd__:zz");
        assert_eq!(decompress_payload("__zstd__:"), "__zstd__:");
        // Empty-queue fetch returns empty.
        let fresh = soshal_test_util::test_db();
        assert!(fetch_pending_outbox_items(&fresh, 0, 0).unwrap().is_empty());
    }
}
