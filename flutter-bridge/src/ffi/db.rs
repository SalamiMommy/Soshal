//! Database FFI module
//!
//! Owns the shared bundled-SQLite [`soshal_db_core::Database`] instance for
//! the Flutter app. `db_init` must run once at startup (with migrations);
//! all other domains query through this connection. Raw SQL helpers are kept
//! for the sparse Dart service layer; domain logic lives in the Rust cores.

use flutter_rust_bridge::frb;
use libsql::Value;
use soshal_db_core::block_on;
use soshal_db_core::error::DbError;
use soshal_db_core::Database;
use std::sync::Mutex;

static DB: Mutex<Option<Database>> = Mutex::new(None);

fn with_db<T>(f: impl FnOnce(&Database) -> Result<T, DbError>) -> Result<T, String> {
    let db = {
        let guard = DB.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(db) => db.clone(),
            None => return Err("database not initialized".to_string()),
        }
    };
    match f(&db) {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("db: {e}")),
    }
}

/// The current database path (set by `db_init`), used as the base directory
/// for the session file and caches.
static DB_PATH: Mutex<Option<String>> = Mutex::new(None);

/// Initialize (or re-initialize) the database at `db_path`, applying all
/// schema migrations. Safe to call once per app start.
#[frb(sync, serialize)]
pub fn db_init(db_path: String) -> Result<String, String> {
    let db = match Database::open(&db_path) {
        Ok(d) => d,
        Err(e) => return Err(format!("open failed: {e}")).into(),
    };
    if let Err(e) = db.migrate() {
        return Err(format!("migration failed: {e}")).into();
    }
    *DB_PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(db_path.clone());
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
    Ok(db_path).into()
}

/// Get the current database path, or an error if not initialized.
#[frb(sync, serialize)]
pub fn db_path() -> Result<String, String> {
    let guard = DB_PATH.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(p) => Ok(p.clone()).into(),
        None => Err("database not initialized".to_string()).into(),
    }
}

/// Execute a raw SELECT query; rows are returned as a JSON array of objects
/// (column names as keys). Parameter binding is supported with `?1..?N`.
#[frb(sync, serialize)]
pub fn db_query_raw(sql: String) -> Result<String, String> {
    with_db(|db| {
        let conn = db.conn().map_err(DbError::from)?;
        let out = block_on(async {
            let mut stmt = conn.prepare(&sql).await?;
            let mut rows = stmt.query(()).await?;
            let names: Vec<String> = stmt
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect();
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                let mut obj = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let val = match row.get_value(i as i32) {
                        Ok(Value::Null) => serde_json::Value::Null,
                        Ok(Value::Integer(n)) => serde_json::json!(n),
                        Ok(Value::Real(r)) => serde_json::json!(r),
                        Ok(Value::Text(t)) => serde_json::json!(t),
                        Ok(Value::Blob(b)) => serde_json::json!(hex::encode(b)),
                        Err(_) => serde_json::Value::Null,
                    };
                    obj.insert(name.clone(), val);
                }
                out.push(serde_json::Value::Object(obj));
            }
            Ok::<_, libsql::Error>(out)
        })?;
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
    })
}

/// Execute a raw INSERT/UPDATE/DELETE (no parameters case); returns rows
/// affected.
#[frb(sync, serialize)]
pub fn db_execute_raw(sql: String) -> Result<usize, String> {
    with_db(|db| {
        let conn = db.conn().map_err(DbError::from)?;
        Ok(block_on(async { conn.execute(&sql, ()).await })? as usize)
    })
}

/// Get the row count of a table.
#[frb(sync, serialize)]
pub fn db_count(table: String) -> Result<i64, String> {
    with_db(|db| {
        let conn = db.conn().map_err(DbError::from)?;
        let sql = format!("SELECT COUNT(*) AS c FROM {table}");
        let count = soshal_db_core::query::query_first(&conn, &sql, (), |r| {
            let n: i64 = r.get(0)?;
            Ok(n)
        })?;
        Ok(count.unwrap_or(0))
    })
}

/// Set a key/value setting (sidebar order, theme, stealth whitelist, PIN
/// flags). Upserts into the `settings` table.
#[frb(sync, serialize)]
pub fn db_set_setting(key: String, value: String) -> Result<bool, String> {
    with_db(|db| {
        soshal_db_core::repos::settings::SettingsRepo::new(db).set(&key, &value)?;
        Ok(true)
    })
}

/// Get a setting value by key.
#[frb(sync, serialize)]
pub fn db_get_setting(key: String) -> Result<Option<String>, String> {
    with_db(|db| {
        soshal_db_core::repos::settings::SettingsRepo::new(db)
            .get(&key)
            .map(|v| v.filter(|s| !s.is_empty()))
    })
}

/// Delete a setting key.
#[frb(sync, serialize)]
pub fn db_delete_setting(key: String) -> Result<bool, String> {
    with_db(|db| {
        soshal_db_core::repos::settings::SettingsRepo::new(db).delete(&key)?;
        Ok(true)
    })
}

/// Storage usage snapshot: db file size + row counts for the main tables
/// (mirrors the legacy `db_get_storage_stats` command).
#[frb(sync, serialize)]
pub fn db_storage_stats() -> Result<String, String> {
    let json = db_query_raw(
        "SELECT name AS table_name, (SELECT COUNT(*) FROM pragma_table_info(name)) AS cols, \
         CASE name \
           WHEN 'posts' THEN (SELECT COUNT(*) FROM posts WHERE is_deleted=0) \
           WHEN 'messages' THEN (SELECT COUNT(*) FROM messages) \
           WHEN 'group_messages' THEN (SELECT COUNT(*) FROM group_messages) \
           WHEN 'notifications' THEN (SELECT COUNT(*) FROM notifications) \
           ELSE 0 END AS rows \
         FROM sqlite_master WHERE type='table' ORDER BY name"
            .to_string(),
    )?;
    let file_bytes = DB_PATH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let mut out: serde_json::Value =
        serde_json::from_str(&json).unwrap_or_else(|_| serde_json::json!([]));
    if let serde_json::Value::Array(arr) = &mut out {
        arr.push(serde_json::json!({
            "table_name": "__db_file__",
            "cols": 0,
            "rows": 0,
            "bytes": file_bytes
        }));
    }
    Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
}

/// Checkpoint the WAL and copy the database file to `backup_path` (a full
/// file snapshot — the only state SQLite needs for a consistent restore).
#[frb(sync, serialize)]
pub fn db_backup(backup_path: String) -> Result<String, String> {
    with_db(|db| {
        let conn = db.conn().map_err(DbError::from)?;
        let _ = block_on(conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);"));
        let src = DB_PATH
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| DbError::NotFound)?;
        std::fs::copy(&src, &backup_path)
            .map_err(|e| DbError::Migration(format!("copy failed: {e}")))?;
        Ok(backup_path)
    })
}

/// Restore: close the current handle, replace the file, and reopen with
/// migrations. Any in-flight connection is dropped.
#[frb(sync, serialize)]
pub fn db_restore(backup_path: String) -> Result<String, String> {
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let dst = {
        let guard = DB_PATH.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .clone()
            .ok_or_else(|| "database not initialized".to_string())?
    };
    if let Err(e) = std::fs::copy(&backup_path, &dst) {
        // re-open the original db so the app stays usable
        if let Ok(db) = Database::open(&dst) {
            *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
        }
        return Err(format!("restore copy failed: {e}")).into();
    }
    match Database::open(&dst) {
        Ok(db) => {
            if let Err(e) = db.migrate() {
                return Err(format!("restore migrate failed: {e}")).into();
            }
            *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
            Ok(dst).into()
        }
        Err(e) => Err(format!("restore open failed: {e}")).into(),
    }
}

/// Run a closure against the shared database. Used by domain modules
/// (feed, messaging, search, …) for repo-backed queries.
pub(crate) fn with_db_result<T>(
    f: impl FnOnce(&Database) -> Result<T, DbError>,
) -> Result<T, String> {
    with_db(f)
}

/// Same as [`with_db_result`] but the closure returns a plain `String` error
/// that is passed through verbatim (no `db:` prefix).
pub(crate) fn with_db_string<T>(
    f: impl FnOnce(&Database) -> Result<T, String>,
) -> Result<T, String> {
    with_db(|db| f(db).map_err(DbError::Migration))
        .map_err(|e| e.trim_start_matches("db: migration: ").to_string())
}

/// Get custom profile nodes for a user from the database.
#[frb(sync, serialize)]
pub fn db_get_custom_profile_nodes(pubkey: String) -> Result<String, String> {
    with_db(|db| {
        let conn = db.conn().map_err(DbError::from)?;
        let sql = "SELECT data FROM custom_profiles WHERE pubkey = ?";
        let pubkey_str = pubkey.as_str();
        let data = soshal_db_core::query::query_first(&conn, sql, [pubkey_str], |r| {
            let data: String = r.get(0)?;
            Ok(data)
        })?;
        Ok(data.unwrap_or_else(|| "[]".to_string()))
    })
}

/// Save custom profile for a user to the database.
#[frb(sync, serialize)]
pub fn db_save_custom_profile(pubkey: String, profile_json: String) -> Result<bool, String> {
    with_db(|db| {
        let conn = db.conn().map_err(DbError::from)?;
        let sql = "INSERT INTO custom_profiles (pubkey, data) VALUES (?, ?)
                   ON CONFLICT(pubkey) DO UPDATE SET data = ?";
        block_on(async {
            conn.execute(sql, [pubkey.as_str(), profile_json.as_str()])
                .await
        })
        .map_err(DbError::from)?;
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_query_roundtrip() {
        let path = format!(
            "{}/soshal_test_{}.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        let init = db_init(path.clone());
        assert!(init.is_ok());
        let exec = db_execute_raw(
            "INSERT INTO users (pubkey, npub, name) VALUES ('abc', 'npub1abc', 'tester') ON CONFLICT DO UPDATE SET name='tester'".to_string(),
        );
        assert!(exec.is_ok());
        let rows = db_query_raw("SELECT pubkey, name FROM users WHERE pubkey='abc'".to_string());
        assert!(rows.is_ok());
        let json = rows.unwrap();
        assert!(json.contains("tester"));
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
    }
}
