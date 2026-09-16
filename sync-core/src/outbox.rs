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

/// Marker prefix for at-rest-sealed outbox payloads. The bridge layer owns
/// the at-rest key and supplies the seal/unseal closures; this crate only
/// recognizes the envelope so payloads can be sealed on write and unsealed
/// on read without ever importing the key.
pub const SEAL_PREFIX: &str = "__seal_b64__:";

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
    if !raw.starts_with("__zstd") {
        return raw.to_string();
    }
    const MAX_DECOMPRESSED: u64 = 1024 * 1024;
    let cap = |bytes: &[u8]| -> Option<String> {
        let decoder = zstd::stream::read::Decoder::new(bytes).ok()?;
        let mut out = Vec::with_capacity(bytes.len() * 3);
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
    enqueue_outbox_item_with_seal(db, id, action_type, payload_json, media_path, now_secs, Ok)
}

/// Enqueue with the payload sealed before storage. `seal` receives the
/// raw/compressed payload and must return the stored form (e.g. the bridge's
/// at-rest `__seal_b64__:` envelope). Honored reverse order at read time:
/// `decompress_payload` first, then `unseal_payload`.
pub fn enqueue_outbox_item_with_seal(
    db: &Database,
    id: &str,
    action_type: &str,
    payload_json: &str,
    media_path: Option<&str>,
    now_secs: i64,
    seal: impl Fn(String) -> Result<String, String>,
) -> Result<(), String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    // Compress THEN seal: ciphertext is ~incompressible, so the compression
    // must happen on the plaintext first to be useful.
    let compressed = compress_payload(payload_json);
    let stored_payload =
        seal(compressed).map_err(|e| format!("Failed to seal outbox payload: {e}"))?;
    block_on(conn.execute(
        "INSERT INTO outbox_queue (id, action_type, payload_json, media_path, status, retry_count, next_retry_at, created_at)
         VALUES (?1, ?2, ?3, ?4, 'pending', 0, 0, ?5)
         ON CONFLICT(id) DO UPDATE SET
             action_type = excluded.action_type,
             payload_json = excluded.payload_json,
             media_path = excluded.media_path,
             status = 'pending',
             retry_count = 0,
             next_retry_at = 0,
             created_at = excluded.created_at",
        params![id, action_type, stored_payload, media_path, now_secs],
    ))
    .map_err(|e| format!("Failed to enqueue outbox item: {e}"))?;
    Ok(())
}

/// Reverse of the seal applied at enqueue: strips the `__seal_b64__:` envelope
/// and hands the inner blob to the caller-supplied `unseal` closure (the
/// secret-holding layer), then restores any zstd compression applied inside
/// the envelope by `enqueue_outbox_item_with_seal` (compress-then-seal; the
/// stored plaintext is the compressed form). Unseal failures fall back to the
/// original string so legacy plaintext rows and transient signer-lock states
/// degrade to today's behavior (parse attempt → retry) instead of throwing.
pub fn unseal_payload(payload: &str, unseal: &impl Fn(&str) -> Result<String, String>) -> String {
    match payload.strip_prefix(SEAL_PREFIX) {
        Some(inner) => match unseal(inner) {
            Ok(plain) => decompress_payload(&plain),
            Err(_) => payload.to_string(),
        },
        None => payload.to_string(),
    }
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
    fetch_pending_outbox_items_with_unseal(
        db,
        now_secs,
        limit,
        include_media,
        |p| Ok(p.to_string()),
    )
}

/// Pending-item fetch with the sealed payload stage: after zstd decompression
/// the `__seal_b64__:` envelope is unwrapped via the caller's `unseal`
/// closure before the payload is surfaced to the publisher.
pub fn fetch_pending_outbox_items_with_unseal(
    db: &Database,
    now_secs: i64,
    limit: usize,
    include_media: bool,
    unseal: impl Fn(&str) -> Result<String, String>,
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
        let stmt = conn.prepare(sql).await.map_err(|e| e.to_string())?;

        let mut rows = stmt
            .query(params![now_secs, limit as i64])
            .await
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| e.to_string())? {
            let raw_payload: String = row.get(2).map_err(|e| e.to_string())?;
            let plain = decompress_payload(&raw_payload);
            out.push(OutboxItem {
                id: row.get(0).map_err(|e| e.to_string())?,
                action_type: row.get(1).map_err(|e| e.to_string())?,
                payload_json: unseal_payload(&plain, &unseal),
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
    mark_settled_dirty();
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
    if status == "failed" {
        mark_settled_dirty();
    }
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

/// Set whenever an outbox row transitions to a settled state (completed /
/// failed). The engine only runs the (full-table) prune pass after such a
/// change, instead of every flush tick.
static SETTLED_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn mark_settled_dirty() {
    SETTLED_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Takes and clears the settled-dirty flag. Returns true when a settle
/// happened since the last prune.
pub fn settled_dirty_take() -> bool {
    SETTLED_DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed)
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
    mark_settled_dirty();
    Ok(())
}

/// Prune settled outbox rows (completed / failed) that are no longer needed.
/// Keeps the newest `keep_latest` settled rows and drops everything older,
/// bounding queue growth for long-running installs. Returns rows removed.
pub fn prune_outbox_settled(db: &Database, keep_latest: u32) -> Result<u64, String> {
    let conn = db.conn().map_err(|e| e.to_string())?;
    let deleted = block_on(conn.execute(
        "DELETE FROM outbox_queue
         WHERE status IN ('completed', 'failed')
           AND id NOT IN (
             SELECT id FROM outbox_queue
             WHERE status IN ('completed', 'failed')
             ORDER BY created_at DESC
             LIMIT ?1
           )",
        params![keep_latest as i64],
    ))
    .map_err(|e| format!("prune outbox: {e}"))?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_outbox_seal_roundtrip() {
        let db = soshal_test_util::test_db();
        // Fake seal: reverse the bytes (an asymmetric-opaque ciphertext
        // stand-in) so non-unsealing readers cannot parse the payload.
        let seal = |s: String| -> Result<String, String> {
            let inner = base64_encode(&s);
            Ok(format!("{SEAL_PREFIX}{inner}"))
        };
        let unseal = move |s: &str| -> Result<String, String> {
            let bytes = base64_decode(s)?;
            String::from_utf8(bytes).map_err(|e| e.to_string())
        };
        let payload = serde_json::json!({"kind": 1, "content": "draft secret"}).to_string();
        enqueue_outbox_item_with_seal(&db, "sealed1", "post", &payload, None, 100, seal).unwrap();
        // Identity fetch (no unseal) surfaces the sealed form — the old
        // behavior would leak the plaintext.
        let raw = fetch_pending_outbox_items(&db, 100, 10).unwrap();
        assert_ne!(
            raw[0].payload_json, payload,
            "sealed payload must not surface as plaintext"
        );
        assert!(
            !raw[0].payload_json.contains("draft secret"),
            "plaintext leaked"
        );
        // Unsealed fetch roundtrips the original payload.
        let sealed = fetch_pending_outbox_items_with_unseal(&db, 100, 10, false, unseal).unwrap();
        assert_eq!(sealed[0].payload_json, payload);
        // Large payload (>256 B) roundtrips too: zstd-compressed inside the
        // seal envelope, decompressed again after unseal.
        let big = serde_json::json!({"kind": 1, "content": "x".repeat(600)}).to_string();
        let seal2 = |s: String| -> Result<String, String> {
            let inner = base64_encode(&s);
            Ok(format!("{SEAL_PREFIX}{inner}"))
        };
        enqueue_outbox_item_with_seal(&db, "sealed-big", "post", &big, None, 100, seal2).unwrap();
        let big_out = fetch_pending_outbox_items_with_unseal(&db, 100, 10, false, unseal).unwrap();
        assert!(
            big_out
                .iter()
                .any(|i| i.id == "sealed-big" && i.payload_json == big),
            "large sealed payload must roundtrip; got {:?}",
            big_out
                .iter()
                .find(|i| i.id == "sealed-big")
                .map(|i| &i.payload_json)
        );
        // Legacy plaintext rows still read through the identity path.
        enqueue_outbox_item(&db, "plain1", "post", "{}", None, 100).unwrap();
        let mixed = fetch_pending_outbox_items(&db, 100, 10).unwrap();
        assert_eq!(mixed.len(), 3);
        assert!(
            mixed.iter().any(|i| i.payload_json == "{}"),
            "legacy row must stay readable"
        );
    }

    fn base64_encode(s: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|e| e.to_string())
    }

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

    #[test]
    fn test_outbox_re_enqueue_resets_pending() {
        let db = soshal_test_util::test_db();
        // Initial enqueue
        enqueue_outbox_item(&db, "item-1", "post", "payload 1", None, 1000).unwrap();
        let items = fetch_pending_outbox_items(&db, 1000, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "item-1");

        // Mark failed with retries
        mark_outbox_item_failed(&db, "item-1", 5, 2000).unwrap();
        let items = fetch_pending_outbox_items(&db, 1500, 10).unwrap();
        assert!(items.is_empty(), "backoff should exclude item");

        // Re-enqueuing the same ID should succeed (not crash with UNIQUE constraint error)
        // and reset status to pending with 0 retries
        enqueue_outbox_item(&db, "item-1", "post", "updated payload", None, 1600).unwrap();
        let items = fetch_pending_outbox_items(&db, 1600, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].retry_count, 0);
        assert_eq!(items[0].status, "pending");
        assert_eq!(items[0].payload_json, "updated payload");
    }
}
