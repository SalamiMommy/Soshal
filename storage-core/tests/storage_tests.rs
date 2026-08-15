//! Integration tests for soshal-storage-core.

use soshal_storage_core::audio_waveform::{extract_peaks, extract_peaks_u8};
use soshal_storage_core::backup_export::{export_envelope, row_to_json, EXPORT_TABLES};
use soshal_storage_core::backup_restore::{
    backup_post_event_payload, backup_post_meta, unwrap_backup_db,
};
use soshal_storage_core::cuckoo_cache::ChunkCuckooFilter;
use soshal_storage_core::erasure_fountain::{decode_fountain, encode_fountain};
use soshal_storage_core::eviction::execute_incremental_vacuum;
use soshal_storage_core::flash_wal::{configure_flash_pragmas, FlashWalFlusher};
use soshal_storage_core::offline_sync::decrypt_offline_sync_json;
use soshal_storage_core::util::{fail_json, hex_to_32_bytes};
use soshal_storage_core::zram_cache::ZramCacheManager;

// ---------------------------------------------------------------------------
// Eviction
// ---------------------------------------------------------------------------

#[test]
fn incremental_vacuum_frees_deleted_pages() {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    soshal_db_core::block_on(conn.execute_batch(
        "PRAGMA auto_vacuum = INCREMENTAL;
         CREATE TABLE t (a INTEGER, b TEXT);",
    ))
    .unwrap();
    for i in 0..2000 {
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO t VALUES (?, ?);",
            libsql::params![i, format!("row{i}")],
        ))
        .unwrap();
    }
    let pages_before = pragma_int(&conn, "PRAGMA page_count;");
    soshal_db_core::block_on(conn.execute("DELETE FROM t;", ())).unwrap();
    let free_before = pragma_int(&conn, "PRAGMA freelist_count;");
    assert!(free_before > 0);
    assert!(execute_incremental_vacuum(&conn, free_before as u32).is_err());
    soshal_db_core::block_on(async {
        let sql = format!("PRAGMA incremental_vacuum({});", free_before as u32);
        let mut stmt = conn.prepare(&sql).await.unwrap();
        let mut rows = stmt.query(()).await.unwrap();
        while rows.next().await.unwrap().is_some() {}
    });
    assert_eq!(pragma_int(&conn, "PRAGMA freelist_count;"), 0);
    assert!(pragma_int(&conn, "PRAGMA page_count;") < pages_before);
}

// ---------------------------------------------------------------------------
// Util
// ---------------------------------------------------------------------------

#[test]
fn fail_json_builds_error_envelope() {
    let out = fail_json("ciphertext_json", "boom");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["error"], "boom");
    assert!(v["ciphertext_json"].is_null());
}

#[test]
fn hex_to_32_bytes_validation() {
    let ok = hex_to_32_bytes(&"ab".repeat(32)).unwrap();
    assert_eq!(ok.len(), 32);
    assert!(hex_to_32_bytes(&"ab".repeat(31)).is_err());
    assert!(hex_to_32_bytes(&"ab".repeat(33)).is_err());
    assert!(hex_to_32_bytes("zz").is_err());
    assert!(hex_to_32_bytes("").is_err());
}

// ---------------------------------------------------------------------------
// Backup export / restore roundtrip
// ---------------------------------------------------------------------------

#[test]
fn export_envelope_builds_versioned_payload() {
    let env = export_envelope(
        1234,
        &[
            ("posts", vec![serde_json::json!({"id": "p1"})]),
            ("users", vec![]),
        ],
    );
    assert_eq!(env["exportedAt"], 1234u64);
    assert_eq!(env["version"], "1.0.0");
    assert_eq!(env["data"]["posts"][0]["id"], "p1");
    assert_eq!(env["data"]["users"].as_array().unwrap().len(), 0);
}

#[test]
fn export_tables_has_expected_order() {
    assert_eq!(
        EXPORT_TABLES,
        [
            "users",
            "posts",
            "reactions",
            "zaps",
            "notifications",
            "bookmarks"
        ]
    );
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

#[test]
fn row_to_json_maps_columns() {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    soshal_db_core::block_on(conn.execute_batch(
        "CREATE TABLE t (id INTEGER, name TEXT, val REAL, data BLOB, unused TEXT);
         INSERT INTO t VALUES (7, 'x', 1.5, X'00FF', NULL);",
    ))
    .unwrap();
    let cols = vec![
        "id".to_string(),
        "name".to_string(),
        "val".to_string(),
        "data".to_string(),
        "unused".to_string(),
    ];
    let out = soshal_db_core::block_on(async {
        let mut stmt = conn.prepare("SELECT * FROM t").await.unwrap();
        let mut rows = stmt.query(()).await.unwrap();
        match rows.next().await.unwrap() {
            Some(row) => row_to_json(&row, &cols),
            None => panic!("no row"),
        }
    });
    assert_eq!(out["id"], 7);
    assert_eq!(out["name"], "x");
    assert_eq!(out["val"], 1.5);
    assert!(out["unused"].is_null());
    let decoded = soshal_crypto_core::base64::base64_decode_bytes(out["data"].as_str().unwrap());
    assert_eq!(decoded.unwrap(), vec![0x00u8, 0xFF]);
}

// ---------------------------------------------------------------------------
// Backup post extraction
// ---------------------------------------------------------------------------

#[test]
fn backup_post_event_payload_builds_event_json() {
    let row = serde_json::json!({
        "id": "e1", "pubkey": "pk1", "content": "hello",
        "created_at": 100, "kind": 1,
        "tags_json": r#"[["t","x"]]"#,
        "sig": "signature"
    });
    let out = backup_post_event_payload(&row).unwrap();
    assert_eq!(out["id"], "e1");
    assert_eq!(out["sig"], "signature");
    assert_eq!(out["tags"][0][0], "t");
    assert_eq!(out["created_at"], 100);
}

#[test]
fn backup_post_event_payload_none_without_id_or_sig() {
    assert!(backup_post_event_payload(&serde_json::json!({"id": "e1"})).is_none());
    assert!(backup_post_event_payload(&serde_json::json!({"id": "", "sig": "s"})).is_none());
    assert!(backup_post_event_payload(
        &serde_json::json!({"id": "e1", "sig": "s", "tags_json": "bad["})
    )
    .is_some());
}

#[test]
fn backup_post_meta_maps_fields_with_defaults() {
    let row = serde_json::json!({
        "reply_to": "r", "root_id": "root", "mentioned_pubkeys": "[]",
        "mentioned_hashtags": "[]", "subject": "subj", "is_deleted": true, "scheduled_at": 42
    });
    let meta = backup_post_meta(&row);
    assert_eq!(meta.reply_to.as_deref(), Some("r"));
    assert_eq!(meta.root_id.as_deref(), Some("root"));
    assert_eq!(meta.subject.as_deref(), Some("subj"));
    assert!(meta.is_deleted);
    assert_eq!(meta.scheduled_at, Some(42));
    assert_eq!(meta.mentioned_pubkeys, "[]");

    let meta2 = backup_post_meta(&serde_json::json!({}));
    assert!(meta2.reply_to.is_none());
    assert!(meta2.root_id.is_none());
    assert!(meta2.subject.is_none());
    assert!(!meta2.is_deleted);
    assert_eq!(meta2.scheduled_at, None);
    assert_eq!(meta2.mentioned_pubkeys, "[]");
}

#[test]
fn unwrap_backup_db_parses_success_envelope() {
    let out = r#"{"success":true,"db_json":"{\"a\":1}"}"#;
    let v = unwrap_backup_db(out).unwrap();
    assert_eq!(v["a"], 1);
}

#[test]
fn unwrap_backup_db_errors_on_failure() {
    let out = r#"{"success":false,"error":"boom"}"#;
    let err = unwrap_backup_db(out).unwrap_err();
    assert_eq!(err, "boom");
    assert!(unwrap_backup_db("garbage").is_err());
}

// ---------------------------------------------------------------------------
// Audio waveform
// ---------------------------------------------------------------------------

#[test]
fn extract_peaks_bins_samples() {
    let samples = vec![-0.5f32, 1.0, 0.25, -1.0];
    assert_eq!(extract_peaks(&samples, 2), vec![1.0, 1.0]);
    assert_eq!(extract_peaks(&samples, 4), vec![0.5, 1.0, 0.25, 1.0]);
    assert_eq!(extract_peaks(&[], 4), vec![] as Vec<f32>);
    assert_eq!(extract_peaks(&samples, 0), vec![] as Vec<f32>);
}

#[test]
fn extract_peaks_u8_scales_to_byte_range() {
    let samples = vec![0.0f32, 1.0, 0.5];
    assert_eq!(extract_peaks_u8(&samples, 3), vec![0, 255, 127]);
    assert_eq!(extract_peaks_u8(&[], 3), vec![] as Vec<u8>);
}

// ---------------------------------------------------------------------------
// Offline sync
// ---------------------------------------------------------------------------

#[test]
fn offline_sync_rejects_missing_signature_on_decrypt() {
    let input = serde_json::json!({
        "envelope_json": r#"{"pqc_ct":"aa","inner":"bb","sender_pubkey":"alice","sender_dsa_pubkey":"cc"}"#,
        "context": "f2f-test",
        "kem_secret_key_hex": "ab".repeat(32),
        "dsa_public_key_hex": "cc",
        "sender_pubkey": "alice"
    });
    let v: serde_json::Value =
        serde_json::from_str(&decrypt_offline_sync_json(&input.to_string())).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["error"], "missing DSA signature");
}

// ---------------------------------------------------------------------------
// Erasure fountain
// ---------------------------------------------------------------------------

fn fountain_payload() -> Vec<u8> {
    (0..25_600u32).map(|i| (i % 251) as u8).collect()
}

#[test]
fn erasure_fountain_drop_30_percent_still_reconstructs() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let dropped = (encoded.packets.len() as f32 * 0.3).ceil() as usize;
    let available = encoded.packets[..encoded.packets.len() - dropped].to_vec();
    let decoded = decode_fountain(&encoded.manifest, &available).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn erasure_fountain_any_k_packets_reconstruct() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let k = encoded.manifest.num_source_symbols as usize;
    let subset = encoded.packets[..k].to_vec();
    let decoded = decode_fountain(&encoded.manifest, &subset).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn erasure_fountain_insufficient_packets_errors() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let few = encoded.packets[..encoded.manifest.num_source_symbols as usize / 2].to_vec();
    let err = decode_fountain(&encoded.manifest, &few).unwrap_err();
    assert!(err.contains("Insufficient"));
}

#[test]
fn erasure_fountain_empty_payload_rejected() {
    assert!(encode_fountain(&[], 0.5).is_err());
}

#[test]
fn erasure_fountain_corrupted_packet_not_detected() {
    let original = fountain_payload();
    let encoded = encode_fountain(&original, 0.5).unwrap();
    let mut packets = encoded.packets.clone();
    packets[0][4 + 17] ^= 0xFF;
    let decoded = decode_fountain(&encoded.manifest, &packets).unwrap();
    assert_ne!(decoded, original);
}

// ---------------------------------------------------------------------------
// Cuckoo cache
// ---------------------------------------------------------------------------

#[test]
fn chunk_cuckoo_insert_hit_delete_miss() {
    let filter = ChunkCuckooFilter::new(100);
    let a = [7u8; 32];
    let b = [9u8; 32];
    assert!(!filter.contains(&a));
    filter.insert(&a).unwrap();
    assert!(filter.contains(&a));
    assert!(!filter.contains(&b));
    assert!(filter.delete(&a));
    assert!(!filter.contains(&a));
}

#[test]
fn chunk_cuckoo_batch_insert_reports_inserted_count() {
    let filter = ChunkCuckooFilter::new(100);
    let hashes: Vec<[u8; 32]> = (1..=5).map(|i| [i as u8; 32]).collect();
    assert_eq!(filter.insert_batch(&hashes).unwrap(), 5);
    for h in &hashes {
        assert!(filter.contains(h));
    }
    assert_eq!(filter.insert_batch(&[]).unwrap(), 0);
}

// ---------------------------------------------------------------------------
// Flash WAL
// ---------------------------------------------------------------------------

#[test]
fn flash_wal_flush_persists_and_reads_back() {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    let mut flusher = FlashWalFlusher::new();
    flusher.push_sql("CREATE TABLE wal_t (id INTEGER);");
    flusher.push_sql("INSERT INTO wal_t VALUES (1);");
    flusher.push_sql("INSERT INTO wal_t VALUES (2);");
    assert_eq!(flusher.flush_to_db(&conn).unwrap(), 3);
    assert_eq!(table_count(&conn, "wal_t"), 2);
}

#[test]
fn flash_wal_flush_empty_buffer_returns_zero() {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    let mut flusher = FlashWalFlusher::new();
    assert_eq!(flusher.flush_to_db(&conn).unwrap(), 0);
}

#[test]
fn flash_wal_batch_tolerates_bad_statement() {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    let mut flusher = FlashWalFlusher::new();
    flusher.push_sql("CREATE TABLE wal_t (id INTEGER);");
    flusher.push_sql("INSERT INTO wal_t VALUES (1);");
    flusher.push_sql("NOT VALID SQL;");
    flusher.push_sql("INSERT INTO wal_t VALUES (2);");
    assert_eq!(flusher.flush_to_db(&conn).unwrap(), 4);
    assert_eq!(table_count(&conn, "wal_t"), 2);
}

#[test]
fn flash_wal_pragmas_set_flash_friendly_values() {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    configure_flash_pragmas(&conn).unwrap();
    assert_eq!(pragma_int(&conn, "PRAGMA page_size;"), 8192);
    assert_eq!(pragma_int(&conn, "PRAGMA auto_vacuum;"), 2);
    assert_eq!(pragma_int(&conn, "PRAGMA wal_autocheckpoint;"), 1000);
}

// ---------------------------------------------------------------------------
// ZRAM cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn zram_roundtrip_preserves_json() {
    let cache = ZramCacheManager::new();
    let payload = r#"{"post_id":"z1","content":"compressed roundtrip"}"#;
    cache.put("k", payload).await;
    assert_eq!(cache.get("k").await.as_deref(), Some(payload));
}

#[tokio::test]
async fn zram_missing_key_returns_none() {
    let cache = ZramCacheManager::new();
    assert!(cache.get("absent").await.is_none());
}

#[tokio::test]
async fn zram_put_overwrites_existing_key() {
    let cache = ZramCacheManager::new();
    cache.put("k", r#"{"v":1}"#).await;
    cache.put("k", r#"{"v":2}"#).await;
    assert_eq!(cache.get("k").await.as_deref(), Some(r#"{"v":2}"#));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pragma_int(conn: &libsql::Connection, sql: &str) -> i64 {
    soshal_db_core::block_on(async {
        let mut stmt = conn.prepare(sql).await.unwrap();
        let mut rows = stmt.query(()).await.unwrap();
        match rows.next().await.unwrap().unwrap().get_value(0).unwrap() {
            libsql::Value::Integer(n) => n,
            _ => 0,
        }
    })
}

fn table_count(conn: &libsql::Connection, table: &str) -> i64 {
    soshal_db_core::block_on(async {
        let sql = format!("SELECT COUNT(*) FROM {table};");
        let mut stmt = conn.prepare(&sql).await.unwrap();
        let mut rows = stmt.query(()).await.unwrap();
        match rows.next().await.unwrap().unwrap().get_value(0).unwrap() {
            libsql::Value::Integer(n) => n,
            _ => 0,
        }
    })
}
