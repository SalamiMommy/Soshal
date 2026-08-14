//! Integration tests for soshal-storage-core.

use soshal_storage_core::audio_waveform::{extract_peaks, extract_peaks_u8};
use soshal_storage_core::backup_export::{
    create_encrypted_backup_payload, export_envelope, row_to_json, EXPORT_TABLES,
};
use soshal_storage_core::backup_restore::{
    backup_post_event_payload, backup_post_meta, decrypt_backup_payload, unwrap_backup_db,
};
use soshal_storage_core::eviction::estimate_eviction;
use soshal_storage_core::offline_sync::{decrypt_offline_sync_json, encrypt_offline_sync_json};
use soshal_storage_core::util::{fail_json, hex_to_32_bytes};

// ---------------------------------------------------------------------------
// Eviction
// ---------------------------------------------------------------------------

#[test]
fn estimate_eviction_under_cap_requires_no_deletion() {
    let est = estimate_eviction(1000, 800, 100).unwrap();
    assert_eq!(est.overshoot_bytes, 0);
    assert_eq!(est.posts_to_delete, 0);
    assert_eq!(est.posts_after_eviction, 800);
}

#[test]
fn estimate_eviction_over_cap_rounds_up() {
    let est = estimate_eviction(1000, 1200, 100).unwrap();
    assert_eq!(est.overshoot_bytes, 200);
    assert_eq!(est.posts_to_delete, 3);
    assert_eq!(est.posts_after_eviction, 900);
}

#[test]
fn estimate_eviction_invalid_inputs_return_none() {
    assert!(estimate_eviction(0, 100, 10).is_none());
    assert!(estimate_eviction(-5, 100, 10).is_none());
    assert!(estimate_eviction(100, 200, 0).is_none());
    assert!(estimate_eviction(100, 200, -1).is_none());
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
fn backup_encrypt_decrypt_roundtrip() {
    let (kem_pub, kem_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let db_json = r#"{"posts":[{"id":"p1","content":"hi"}]}"#;
    let input = serde_json::json!({
        "db_json": db_json,
        "kem_public_key_hex": kem_pub
    });
    let out = create_encrypted_backup_payload(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], true);
    let ciphertext_json = v["ciphertext_json"].as_str().unwrap();

    let restore = serde_json::json!({
        "payload": ciphertext_json,
        "sk_hex": kem_sk
    });
    let out = decrypt_backup_payload(&restore.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["db_json"].as_str().unwrap(), db_json);
}

#[test]
fn backup_export_rejects_bad_json() {
    let out = create_encrypted_backup_payload("garbage");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], false);
}

#[test]
fn backup_restore_rejects_unsupported_version() {
    let (kem_pub, kem_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let input = serde_json::json!({
        "db_json": "{}",
        "kem_public_key_hex": kem_pub
    });
    let out = create_encrypted_backup_payload(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_str(v["ciphertext_json"].as_str().unwrap()).unwrap();
    envelope["version"] = serde_json::json!(2);

    let restore = serde_json::json!({"payload": envelope.to_string(), "sk_hex": kem_sk});
    let out = decrypt_backup_payload(&restore.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], false);
    assert!(v["error"]
        .as_str()
        .unwrap()
        .contains("unsupported envelope version"));
}

#[test]
fn backup_restore_rejects_tampered_envelope() {
    let (kem_pub, kem_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
    let input = serde_json::json!({
        "db_json": "{}",
        "kem_public_key_hex": kem_pub
    });
    let out = create_encrypted_backup_payload(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_str(v["ciphertext_json"].as_str().unwrap()).unwrap();
    let mut payload = envelope["payload"].as_str().unwrap().to_string();
    payload.push('A');
    envelope["payload"] = serde_json::json!(payload);

    let restore = serde_json::json!({"payload": envelope.to_string(), "sk_hex": kem_sk});
    let out = decrypt_backup_payload(&restore.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], false);
}

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

fn offline_sync_roundtrip(
    envelope_json: &str,
    kem_sk: &str,
    dsa_pk: &str,
    sender: &str,
) -> serde_json::Value {
    let input = serde_json::json!({
        "envelope_json": envelope_json,
        "context": "f2f-test",
        "kem_secret_key_hex": kem_sk,
        "dsa_public_key_hex": dsa_pk,
        "sender_pubkey": sender
    });
    let out = decrypt_offline_sync_json(&input.to_string());
    serde_json::from_str(&out).unwrap()
}

#[test]
fn offline_sync_encrypt_decrypt_roundtrip() {
    let (kem_pub, kem_sk) = soshal_pqc_core::kem::kem_keygen().unwrap();
    let (dsa_sk, dsa_pk) = soshal_pqc_core::dsa::dsa_keygen(None).unwrap();
    let input = serde_json::json!({
        "payload_json": r#"{"msg":"hi","n":7}"#,
        "peer_pubkey": "peer1",
        "context": "f2f-test",
        "kem_public_key_hex": kem_pub,
        "dsa_secret_key_hex": dsa_sk,
        "sender_pubkey": "alice",
        "sender_dsa_pubkey": dsa_pk
    });
    let out = encrypt_offline_sync_json(&input.to_string());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["success"], true);
    let envelope = v["envelope_json"].as_str().unwrap();

    let dec = offline_sync_roundtrip(envelope, &kem_sk, &dsa_pk, "alice");
    assert_eq!(dec["success"], true);
    let parsed: serde_json::Value =
        serde_json::from_str(dec["payload_json"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["msg"], "hi");
    assert_eq!(parsed["n"], 7);
}

#[test]
fn offline_sync_rejects_wrong_expected_sender() {
    let (kem_pub, kem_sk) = soshal_pqc_core::kem::kem_keygen().unwrap();
    let (dsa_sk, dsa_pk) = soshal_pqc_core::dsa::dsa_keygen(None).unwrap();
    let input = serde_json::json!({
        "payload_json": "{}",
        "peer_pubkey": "peer1",
        "context": "f2f-test",
        "kem_public_key_hex": kem_pub,
        "dsa_secret_key_hex": dsa_sk,
        "sender_pubkey": "alice",
        "sender_dsa_pubkey": dsa_pk
    });
    let v: serde_json::Value =
        serde_json::from_str(&encrypt_offline_sync_json(&input.to_string())).unwrap();
    let envelope = v["envelope_json"].as_str().unwrap();

    let dec = offline_sync_roundtrip(envelope, &kem_sk, &dsa_pk, "mallory");
    assert_eq!(dec["success"], false);
    let err = dec["error"].as_str().unwrap();
    assert_eq!(err, "envelope sender does not match expected sender");
}

#[test]
fn offline_sync_rejects_tampered_envelope() {
    let (kem_pub, kem_sk) = soshal_pqc_core::kem::kem_keygen().unwrap();
    let (dsa_sk, dsa_pk) = soshal_pqc_core::dsa::dsa_keygen(None).unwrap();
    let input = serde_json::json!({
        "payload_json": "{}",
        "peer_pubkey": "peer1",
        "context": "f2f-test",
        "kem_public_key_hex": kem_pub,
        "dsa_secret_key_hex": dsa_sk,
        "sender_pubkey": "alice",
        "sender_dsa_pubkey": dsa_pk
    });
    let v: serde_json::Value =
        serde_json::from_str(&encrypt_offline_sync_json(&input.to_string())).unwrap();
    let mut env: serde_json::Value =
        serde_json::from_str(v["envelope_json"].as_str().unwrap()).unwrap();
    let mut ct = env["pqc_ct"].as_str().unwrap().to_string();
    ct.insert(0, '0');
    env["pqc_ct"] = serde_json::json!(ct);

    let dec = offline_sync_roundtrip(&env.to_string(), &kem_sk, &dsa_pk, "alice");
    assert_eq!(dec["success"], false);
    assert!(dec["error"]
        .as_str()
        .unwrap()
        .contains("DSA signature verification failed"));
}

#[test]
fn offline_sync_requires_sender_pubkeys_on_encrypt() {
    let (kem_pub, _) = soshal_pqc_core::kem::kem_keygen().unwrap();
    let (dsa_sk, dsa_pk) = soshal_pqc_core::dsa::dsa_keygen(None).unwrap();
    let input = serde_json::json!({
        "payload_json": "{}",
        "peer_pubkey": "peer1",
        "context": "f2f-test",
        "kem_public_key_hex": kem_pub,
        "dsa_secret_key_hex": dsa_sk,
        "sender_pubkey": "",
        "sender_dsa_pubkey": dsa_pk
    });
    let v: serde_json::Value =
        serde_json::from_str(&encrypt_offline_sync_json(&input.to_string())).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["error"], "sender_pubkey is required");
}

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
