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

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_conn() -> libsql::Connection {
        let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
        db.connect().unwrap()
    }

    fn first_row(conn: &libsql::Connection, sql: &str, cols: &[String]) -> serde_json::Value {
        soshal_db_core::block_on(async {
            let mut stmt = conn.prepare(sql).await.unwrap();
            let mut rows = stmt.query(()).await.unwrap();
            match rows.next().await.unwrap() {
                Some(row) => row_to_json(&row, cols),
                None => panic!("no row"),
            }
        })
    }

    #[test]
    fn test_export_envelope_produces_valid_blob() {
        let env = export_envelope(
            42,
            &[(
                "posts",
                vec![serde_json::json!({"id": "p1", "content": "hi"})],
            )],
        );
        assert_eq!(env["exportedAt"], 42u64);
        assert_eq!(env["version"], "1.0.0");
        assert_eq!(env["data"]["posts"].as_array().unwrap().len(), 1);
        assert_eq!(env["data"]["posts"][0]["id"], "p1");
        let serialized = serde_json::to_string(&env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed, env);
    }

    #[test]
    fn test_export_envelope_deterministic() {
        let tables = [
            ("users", vec![serde_json::json!({"id": 1, "name": "a"})]),
            ("posts", vec![]),
        ];
        let a = serde_json::to_string(&export_envelope(7, &tables)).unwrap();
        let b = serde_json::to_string(&export_envelope(7, &tables)).unwrap();
        assert_eq!(a, b);
        let c = serde_json::to_string(&export_envelope(8, &tables)).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn test_export_envelope_includes_all_export_tables() {
        let tables: Vec<(&str, Vec<serde_json::Value>)> =
            EXPORT_TABLES.iter().map(|t| (*t, vec![])).collect();
        let env = export_envelope(0, &tables);
        for t in EXPORT_TABLES {
            assert!(env["data"][t].is_array(), "missing table {t}");
        }
        assert_eq!(env["data"].as_object().unwrap().len(), EXPORT_TABLES.len());
    }

    #[test]
    fn test_row_to_json_bad_input_yields_null_no_panic() {
        let conn = in_memory_conn();
        soshal_db_core::block_on(
            conn.execute_batch("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (1);"),
        )
        .unwrap();
        let cols = vec!["a".to_string(), "missing".to_string()];
        let out = first_row(&conn, "SELECT * FROM t", &cols);
        assert_eq!(out["a"], 1);
        assert!(out["missing"].is_null());
    }

    #[test]
    fn test_row_to_json_blob_base64_encoded() {
        let conn = in_memory_conn();
        soshal_db_core::block_on(
            conn.execute_batch("CREATE TABLE t (data BLOB); INSERT INTO t VALUES (X'00FF10');"),
        )
        .unwrap();
        let cols = vec!["data".to_string()];
        let out = first_row(&conn, "SELECT * FROM t", &cols);
        let expected = soshal_crypto_core::base64::base64_encode_bytes(&[0x00u8, 0xFF, 0x10]);
        assert_eq!(out["data"], expected);
    }
}
