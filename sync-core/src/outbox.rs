//! Outbox queue processor and offline background upload engine.

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
            return format!("__zstd__:{}", hex::encode(buf));
        }
    }
    payload.to_string()
}

pub fn decompress_payload(raw: &str) -> String {
    if let Some(hex_data) = raw.strip_prefix("__zstd__:") {
        if let Ok(bytes) = hex::decode(hex_data) {
            if bytes.len() > 4 && &bytes[..4] == ZSTD_MAGIC {
                if let Ok(decompressed) = zstd::decode_all(&bytes[4..]) {
                    if let Ok(s) = String::from_utf8(decompressed) {
                        return s;
                    }
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
    let conn = db.conn().map_err(|e| e.to_string())?;
    block_on(async {
        let mut stmt = conn
            .prepare(
                "SELECT id, action_type, payload_json, media_path, status, retry_count, next_retry_at, created_at
                 FROM outbox_queue
                 WHERE status = 'pending' AND next_retry_at <= ?1
                 ORDER BY created_at ASC
                 LIMIT ?2",
            )
            .await
            .map_err(|e| e.to_string())?;

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
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn mark_outbox_item_failed(
    db: &Database,
    id: &str,
    current_retry: i32,
    now_secs: i64,
) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let new_retry = current_retry + 1;
    let delay_secs = (1i64 << new_retry.min(8)).min(300);
    let next_retry = now_secs + delay_secs;
    let status = if new_retry >= 10 { "failed" } else { "pending" };

    block_on(conn.execute(
        "UPDATE outbox_queue SET status = ?1, retry_count = ?2, next_retry_at = ?3 WHERE id = ?4",
        params![status, new_retry, next_retry, id],
    ))
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_outbox_summary(db: &Database) -> Result<OutboxSummary, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let row = query_first(
        &conn,
        "SELECT COUNT(*), COALESCE(SUM(status = 'pending'), 0), COALESCE(SUM(status = 'failed'), 0) FROM outbox_queue",
        (),
        |r| Ok((r.get(1)?, r.get(2)?, r.get(0)?)),
    )
    .map_err(|e| e.to_string())?;
    let (pending, failed, total) = row.unwrap_or((0, 0, 0));

    Ok(OutboxSummary {
        pending_count: pending.max(0) as u32,
        failed_count: failed.max(0) as u32,
        total_count: total as u32,
    })
}

/// Mark many outbox items completed in a single transaction (one round-trip
/// per item but a single BEGIN/COMMIT instead of N autocommits).
pub fn mark_outbox_items_completed(db: &Database, ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let conn = db.conn().map_err(|e| e.to_string())?;
    soshal_db_core::query::with_tx(&conn, |tx| async move {
        for id in ids {
            tx.execute(
                "UPDATE outbox_queue SET status = 'completed' WHERE id = ?1",
                params![id.as_str()],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    })
    .map_err(|e| format!("batch outbox complete: {e}"))
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
        let summary = get_outbox_summary(&db).unwrap();
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
        mark_outbox_item_failed(&db, "old", 0, 100).unwrap();
        let pending = fetch_pending_outbox_items(&db, 100, 10).unwrap();
        assert_eq!(pending.len(), 1, "retry-gated item excluded");
        assert_eq!(pending[0].id, "new");
        // Limit respected.
        let pending = fetch_pending_outbox_items(&db, 10_000, 1).unwrap();
        assert_eq!(pending.len(), 1);
        // Retry backoff: exponential, capped at 300 s; status flips to
        // failed after 10 attempts.
        mark_outbox_item_failed(&db, "old", 0, 10_000).unwrap();
        mark_outbox_item_failed(&db, "old", 8, 10_300).unwrap();
        let items = fetch_pending_outbox_items(&db, 10_600, 10).unwrap();
        let old = items.iter().find(|i| i.id == "old").unwrap();
        assert_eq!(old.retry_count, 9);
        assert_eq!(
            old.next_retry_at,
            10_300 + 256,
            "backoff capped at 256s (2^8) by design"
        );
        mark_outbox_item_failed(&db, "old", 9, 10_600).unwrap();
        let summary = get_outbox_summary(&db).unwrap();
        assert_eq!(summary.failed_count, 1, "retry cap reached");
        // Batch completion flips the rest.
        mark_outbox_items_completed(&db, &["new".to_string()]).unwrap();
        let summary = get_outbox_summary(&db).unwrap();
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
        // Large payloads roundtrip through zstd.
        let big = "x".repeat(10_000);
        let stored = compress_payload(&big);
        assert!(
            stored.starts_with("__zstd__:"),
            "stored: {}..",
            &stored[..24]
        );
        assert_eq!(decompress_payload(&stored), big);
        // Garbage prefixed payloads degrade to the raw string.
        assert_eq!(decompress_payload("__zstd__:zz"), "__zstd__:zz");
        assert_eq!(decompress_payload("__zstd__:"), "__zstd__:");
        // Empty-queue fetch returns empty.
        let fresh = soshal_test_util::test_db();
        assert!(fetch_pending_outbox_items(&fresh, 0, 0).unwrap().is_empty());
    }
}
