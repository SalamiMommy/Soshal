//! Database FFI module
//!
//! Owns the shared bundled-SQLite [`soshal_db_core::Database`] instance for
//! the Flutter app. `db_init` must run once at startup (with migrations);
//! all other domains query through this connection. Raw SQL helpers are kept
//! for the sparse Dart service layer; domain logic lives in the Rust cores.

use flutter_rust_bridge::frb;
use libsql::params_from_iter;
use libsql::Value;
use soshal_db_core::block_on;
use soshal_db_core::error::DbError;
use soshal_db_core::Database;
use std::sync::Mutex;

static DB: Mutex<Option<Database>> = Mutex::new(None);

/// Tables that `db_count` may count (mirrors the schema migrations).
const COUNTABLE_TABLES: &[&str] = &[
    "audit_logs",
    "banned_members",
    "blocks",
    "bookmarks",
    "custom_profile_nodes",
    "custom_profiles",
    "dating_unmatches",
    "diagnostic_logs",
    "do_not_refetch_items",
    "ephemeral_media",
    "escrows",
    "friend_backups",
    "geohash_peers",
    "group_invites",
    "group_join_requests",
    "group_members",
    "group_messages",
    "group_roles",
    "group_rooms",
    "group_shared_keys",
    "group_thread_replies",
    "group_threads",
    "group_voice_channels",
    "group_voice_presence",
    "groups",
    "guestbook_entries",
    "hashtags",
    "huddle_posts",
    "link_previews",
    "marketplace_reviews",
    "media_blobs",
    "messages",
    "muted_conversations",
    "musicloud_comments",
    "musiclouds",
    "notifications",
    "outbox_queue",
    "poll_votes",
    "polls",
    "post_views",
    "posts",
    "reactions",
    "relays",
    "reminders",
    "reposts",
    "settings",
    "spam_reports",
    "story_reactions",
    "stream_chat",
    "tx_edges",
    "tx_nodes",
    "users",
    "zaps",
];

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
    // rustls 0.23 is built with both the `ring` (reqwest) and `aws-lc-rs`
    // (nostr-sdk) features, so no process-level provider is auto-selected.
    // Install ring unconditionally at startup: without this, any rustls use
    // (reqwest on a tokio worker, quinn, …) panics before the lazy installs
    // in sync-core/network-core run.
    let _ = rustls::crypto::ring::default_provider().install_default();
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

/// Get the current schema version from the _migrations table.
#[frb(sync, serialize)]
pub fn db_schema_version() -> Result<i64, String> {
    with_db(|db| {
        let conn = db.conn()?;
        let version = block_on(async {
            let mut rows = conn
                .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
                .await?;
            if let Some(row) = rows.next().await? {
                Ok::<i64, libsql::Error>(row.get::<i64>(0)?)
            } else {
                Ok(0)
            }
        })?;
        Ok(version)
    })
}

/// The schema version this build's migration runner produces
/// (db-core `SCHEMA_VERSION`). Clients use it to detect stale binaries
/// without hardcoding a copy.
#[frb(sync, serialize)]
pub fn db_expected_schema_version() -> i64 {
    soshal_db_core::schema::SCHEMA_VERSION
}

/// Force re-run all migrations from scratch. This deletes the _migrations table
/// and re-runs the full migration sequence. Use with caution - it may fail if
/// schema changes are not backwards compatible.
#[frb(sync, serialize)]
pub fn db_force_migrate() -> Result<String, String> {
    with_db(|db| {
        let conn = db.conn()?;

        let current: i64 = block_on(async {
            let mut rows = conn
                .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
                .await?;
            if let Some(row) = rows.next().await? {
                Ok::<i64, libsql::Error>(row.get::<i64>(0)?)
            } else {
                Ok(0)
            }
        })?;
        if current >= soshal_db_core::schema::SCHEMA_VERSION {
            return Err(DbError::Migration(format!(
                "refusing to wipe: schema already at version {} (current), expected {}",
                current,
                soshal_db_core::schema::SCHEMA_VERSION
            )));
        }

        let tables: Vec<String> = block_on(async {
            let mut rows = conn.query("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", ()).await?;
            let mut names = Vec::new();
            while let Some(row) = rows.next().await? {
                names.push(row.get::<String>(0)?);
            }
            Ok::<Vec<String>, libsql::Error>(names)
        })?;

        let _ = block_on(conn.execute("PRAGMA foreign_keys = OFF", ()));
        for table in tables {
            let _ = block_on(conn.execute(&format!("DROP TABLE IF EXISTS {}", table), ()));
        }
        let _ = block_on(conn.execute("PRAGMA foreign_keys = ON", ()));

        // Re-run migrations
        db.migrate()
            .map_err(|e| DbError::Migration(format!("migration failed: {e}")))?;
        Ok("Migration re-run complete".to_string())
    })
}

/// Execute a raw SELECT query; rows are returned as a JSON array of objects
/// (column names as keys). Parameter binding is supported with `?1..?N`.
#[frb(sync, serialize)]
pub fn db_query_raw(sql: String) -> Result<String, String> {
    db_query_params(&sql, &[])
}

/// Internal helper: raw SELECT with bound ?N parameters (Vec<String>),
/// rows as a JSON array of objects. Not an FFI surface.
pub fn db_query_params(sql: &str, params: &[String]) -> Result<String, String> {
    with_db(|db| {
        let conn = db.conn()?;
        let out = block_on(async {
            let mut stmt = conn.prepare(sql).await?;
            let mut rows = stmt
                .query(params_from_iter(params.iter().map(|p| p.as_str())))
                .await?;
            rows_json(&stmt, &mut rows).await
        })?;
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
    })
}

async fn rows_json(
    stmt: &libsql::Statement,
    rows: &mut libsql::Rows,
) -> Result<Vec<serde_json::Value>, libsql::Error> {
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
    Ok(out)
}

/// Execute a raw INSERT/UPDATE/DELETE (no parameters); returns rows affected.
#[frb(sync, serialize)]
pub fn db_execute_raw(sql: String) -> Result<usize, String> {
    with_db(|db| {
        let conn = db.conn()?;
        Ok(block_on(async { conn.execute(&sql, ()).await })? as usize)
    })
}

/// Get the row count of a table.
#[frb(sync, serialize)]
pub fn db_count(table: String) -> Result<i64, String> {
    if !COUNTABLE_TABLES.contains(&table.as_str()) {
        return Err(format!("db: unknown table: {table}"));
    }
    with_db(|db| {
        let conn = db.conn()?;
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
    let mut out: Vec<serde_json::Value> = Vec::new();
    with_db(|db| {
        let conn = db.conn()?;
        let names = soshal_db_core::query::query(
            &conn,
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
            (),
            |r| {
                let name: String = r.get(0)?;
                Ok(name)
            },
        )?;
        let table_cols: Vec<(String, i64)> = if names.is_empty() {
            Vec::new()
        } else {
            // Validate every table name before interpolating into SQL.
            // Names come from sqlite_master; a malicious restore file could
            // introduce names with SQL meta-characters.  Only allow
            // identifiers matching [A-Za-z_][A-Za-z0-9_]* .
            let valid_names: Vec<&String> = names
                .iter()
                .filter(|n| {
                    let mut chars = n.chars();
                    match chars.next() {
                        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                        }
                        _ => false,
                    }
                })
                .collect();
            if valid_names.is_empty() {
                Vec::new()
            } else {
                let cols_sql = valid_names
                    .iter()
                    .map(|n| {
                        // n is already validated — no quoting needed, but we
                        // quote defensively for pragma_table_info arg.
                        format!(
                            "SELECT '{}' AS t, COUNT(*) AS c FROM pragma_table_info('{}')",
                            n, n
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" UNION ALL ");
                soshal_db_core::query::query(&conn, &cols_sql, (), |r| {
                    let name: String = r.get(0)?;
                    let cols: i64 = r.get(1)?;
                    Ok((name, cols))
                })?
            }
        };
        for (name, cols) in table_cols {
            let count_sql = match name.as_str() {
                "posts" => "SELECT COUNT(*) FROM posts WHERE is_deleted=0".to_string(),
                "messages" | "group_messages" | "notifications" => {
                    format!("SELECT COUNT(*) FROM {name}")
                }
                _ => String::new(),
            };
            let rows: i64 = if count_sql.is_empty() {
                0
            } else {
                soshal_db_core::query::query_first(&conn, &count_sql, (), |r| {
                    let n: i64 = r.get(0)?;
                    Ok(n)
                })?
                .unwrap_or(0)
            };
            out.push(serde_json::json!({
                "table_name": name,
                "cols": cols,
                "rows": rows,
            }));
        }
        Ok::<_, DbError>(())
    })?;
    let file_bytes = DB_PATH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    out.push(serde_json::json!({
        "table_name": "__db_file__",
        "cols": 0,
        "rows": 0,
        "bytes": file_bytes
    }));
    Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
}

/// Checkpoint the WAL and copy the database file to `backup_path` (a full
/// file snapshot — the only state SQLite needs for a consistent restore).
///
/// `backup_path` must resolve to the same directory as the current database
/// file; paths outside that directory are rejected to prevent path traversal.
#[frb(sync, serialize)]
pub fn db_backup(backup_path: String) -> Result<String, String> {
    // Path-traversal guard: resolve both paths and compare parent dirs.
    let src_path = {
        let guard = DB_PATH.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .clone()
            .ok_or_else(|| "database not initialized".to_string())?
    };
    let db_dir = std::path::Path::new(&src_path)
        .parent()
        .ok_or("cannot determine db directory")?;
    let dest = std::path::Path::new(&backup_path);
    let dest_dir = dest.parent().ok_or("backup path has no parent directory")?;
    // Use canonicalize on the dir (destination file need not exist yet).
    let db_dir_canon =
        std::fs::canonicalize(db_dir).map_err(|e| format!("db dir canonicalize: {e}"))?;
    // dest_dir must already exist for canonicalize to work.
    let dest_dir_canon =
        std::fs::canonicalize(dest_dir).map_err(|e| format!("backup dir canonicalize: {e}"))?;
    if db_dir_canon != dest_dir_canon {
        return Err("backup path must be in the same directory as the database".to_string());
    }
    with_db(|db| {
        let conn = db.conn()?;
        let _ = block_on(conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);"));
        std::fs::copy(&src_path, &backup_path)
            .map_err(|e| DbError::Migration(format!("copy failed: {e}")))?;
        Ok(backup_path.clone())
    })
}

/// Restore: close the current handle, replace the file, and reopen with
/// migrations. Any in-flight connection is dropped.
///
/// `backup_path` must resolve to the same directory as the current database
/// file; paths outside that directory are rejected to prevent path traversal
/// and malicious DB injection.
#[frb(sync, serialize)]
pub fn db_restore(backup_path: String) -> Result<String, String> {
    // Path-traversal guard.
    let dst = {
        let guard = DB_PATH.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .clone()
            .ok_or_else(|| "database not initialized".to_string())?
    };
    let db_dir = std::path::Path::new(&dst)
        .parent()
        .ok_or("cannot determine db directory")?;
    let src_dir = std::path::Path::new(&backup_path)
        .parent()
        .ok_or("backup path has no parent directory")?;
    let db_dir_canon =
        std::fs::canonicalize(db_dir).map_err(|e| format!("db dir canonicalize: {e}"))?;
    let src_dir_canon =
        std::fs::canonicalize(src_dir).map_err(|e| format!("backup dir canonicalize: {e}"))?;
    if db_dir_canon != src_dir_canon {
        return Err("restore path must be in the same directory as the database".to_string());
    }
    // Validate SQLite magic bytes before replacing the live DB.
    const SQLITE_MAGIC: &[u8] = b"SQLite format 3\x00";
    let mut header = [0u8; 16];
    let mut f =
        std::fs::File::open(&backup_path).map_err(|e| format!("cannot open backup file: {e}"))?;
    use std::io::Read;
    f.read_exact(&mut header)
        .map_err(|_| "backup file too small to be a valid SQLite database".to_string())?;
    if header != SQLITE_MAGIC {
        return Err("backup file is not a valid SQLite 3 database".to_string());
    }
    drop(f);
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Err(e) = std::fs::copy(&backup_path, &dst) {
        // re-open the original db so the app stays usable
        if let Ok(db) = Database::open(&dst) {
            *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
        }
        return Err(format!("restore copy failed: {e}"));
    }
    match Database::open(&dst) {
        Ok(db) => {
            if let Err(e) = db.migrate() {
                return Err(format!("restore migrate failed: {e}"));
            }
            *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
            Ok(dst)
        }
        Err(e) => Err(format!("restore open failed: {e}")),
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

pub(crate) fn upsert_post_row(
    id: String,
    pubkey: String,
    content: String,
    kind: i64,
    created_at: i64,
    tags_json: String,
    subject: Option<String>,
) -> Result<(), String> {
    let row = soshal_db_core::repos::post::PostRow {
        id,
        pubkey,
        content,
        kind,
        created_at,
        tags_json,
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: String::new(),
        mentioned_hashtags: String::new(),
        subject,
        sync_status: "pending".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
        rsvp_event_id: None,
    };
    with_db_result(|db| {
        soshal_db_core::repos::post::PostRepo::new(db).upsert(&row)?;
        Ok(())
    })
}

/// Get custom profile nodes for a user from the database.
#[frb(sync, serialize)]
pub fn db_get_custom_profile_nodes(pubkey: String) -> Result<String, String> {
    with_db(|db| {
        let conn = db.conn()?;
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
        let conn = db.conn()?;
        let sql = "INSERT INTO custom_profiles (pubkey, data) VALUES (?, ?)
                   ON CONFLICT(pubkey) DO UPDATE SET data = ?";
        block_on(async {
            conn.execute(
                sql,
                [
                    pubkey.as_str(),
                    profile_json.as_str(),
                    profile_json.as_str(),
                ],
            )
            .await
        })
        .map_err(DbError::from)?;
        Ok(true)
    })
}

/// Delete posts older than `cutoff_secs` (relative to now); returns rows removed.
#[frb(sync, serialize)]
pub fn db_delete_older_than(cutoff_secs: i64) -> Result<usize, String> {
    with_db(|db| {
        use soshal_db_core::repos::post::PostRepo;
        let repo = PostRepo::new(db);
        Ok(repo.delete_older_than(cutoff_secs)? as usize)
    })
}

/// Delete every stored post; returns rows removed.
#[frb(sync, serialize)]
pub fn db_delete_all_posts() -> Result<usize, String> {
    with_db(|db| {
        use soshal_db_core::repos::post::PostRepo;
        let repo = PostRepo::new(db);
        Ok(repo.delete_all_posts()? as usize)
    })
}

/// Trending hashtags by usage count (JSON rows: tag/pubkey/last_used_at/count).
#[frb(sync, serialize)]
pub fn db_get_trending_hashtags(limit: i64) -> Result<String, String> {
    with_db(|db| {
        use soshal_db_core::repos::hashtag::HashtagRepo;
        let repo = HashtagRepo::new(db);
        let rows = repo.get_trending(limit.clamp(1, 100))?;
        let out: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "tag": r.tag,
                    "pubkey": r.pubkey,
                    "last_used_at": r.last_used_at,
                    "count": r.count,
                })
            })
            .collect();
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
    })
}

/// Escrow rows where `pubkey` participates as buyer or seller (JSON).
#[frb(sync, serialize)]
pub fn db_get_escrows_by_participant(pubkey: String) -> Result<String, String> {
    with_db(|db| {
        use soshal_db_core::repos::escrow::EscrowRepo;
        let repo = EscrowRepo::new(db);
        let rows = repo.get_by_participant(&pubkey)?;
        let out: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "listing_id": r.listing_id,
                    "buyer_pubkey": r.buyer_pubkey,
                    "seller_pubkey": r.seller_pubkey,
                    "amount_msats": r.amount_msats,
                    "currency": r.currency,
                    "status": r.status,
                    "escrow_note": r.escrow_note,
                    "created_at": r.created_at,
                    "updated_at": r.updated_at,
                })
            })
            .collect();
        Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string()))
    })
}

/// Purge geohash peer rows not seen within `cutoff_secs_ago`; returns rows removed.
#[frb(sync, serialize)]
pub fn db_purge_stale_geohash_peers(cutoff_secs_ago: i64) -> Result<usize, String> {
    with_db(|db| {
        use soshal_db_core::repos::geohash_peer::GeohashPeerRepo;
        let repo = GeohashPeerRepo::new(db);
        Ok(repo.purge_stale(cutoff_secs_ago)? as usize)
    })
}

#[cfg(test)]
pub(crate) fn tmp_db_path(label: &str, prefix: &str) -> String {
    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = format!(
        "{}/soshal_{prefix}_{label}_{}_{}.db",
        std::env::temp_dir().to_string_lossy(),
        std::process::id(),
        n
    );
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
    path
}

#[cfg(test)]
pub(crate) fn tmp_db(label: &str, prefix: &str) -> String {
    let path = tmp_db_path(label, prefix);
    db_init(path.clone()).unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_query_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
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

    #[test]
    fn test_backup_restore_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = format!(
            "{}/soshal_test_{}_backup.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let backup_path = format!("{path}.bak");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_init(path.clone()).is_ok());
        assert!(db_execute_raw(
            "CREATE TABLE backup_test (id INTEGER PRIMARY KEY, val TEXT)".to_string()
        )
        .is_ok());
        assert!(
            db_execute_raw("INSERT INTO backup_test (val) VALUES ('survivor')".to_string()).is_ok()
        );
        assert!(db_backup(backup_path.clone()).is_ok());
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_restore(backup_path.clone()).is_ok());
        let rows = db_query_raw("SELECT val FROM backup_test".to_string());
        assert!(rows.is_ok());
        assert!(rows.unwrap().contains("survivor"));
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
    }

    #[test]
    fn test_settings_roundtrip() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = format!(
            "{}/soshal_test_{}_settings.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_init(path.clone()).is_ok());
        assert!(db_set_setting("theme".to_string(), "dark".to_string()).is_ok());
        let got = db_get_setting("theme".to_string());
        assert!(got.is_ok());
        assert_eq!(got.unwrap(), Some("dark".to_string()));
        assert!(db_delete_setting("theme".to_string()).is_ok());
        let gone = db_get_setting("theme".to_string());
        assert!(gone.is_ok());
        assert_eq!(gone.unwrap(), None);
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_storage_stats() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = format!(
            "{}/soshal_test_{}_stats.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_init(path.clone()).is_ok());
        assert!(db_execute_raw(
            "INSERT INTO users (pubkey, npub, name) VALUES ('stat1', 'npub1stat', 's') \
                 ON CONFLICT DO UPDATE SET name='s'"
                .to_string()
        )
        .is_ok());
        let stats = db_storage_stats();
        assert!(stats.is_ok());
        assert!(!stats.unwrap().is_empty());
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_delete_older_than() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = format!(
            "{}/soshal_test_{}_feedrepos.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_init(path.clone()).is_ok());
        assert!(db_execute_raw(
            "INSERT INTO users (pubkey, npub, name) VALUES ('pk', 'npub1pk', 't') ON CONFLICT DO NOTHING".to_string()
        )
        .is_ok());
        assert!(db_execute_raw(
            "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sync_status, is_deleted) \
             VALUES ('p1','pk','hello #soshal',1,1000,'[]','synced',0)".to_string()
        )
        .is_ok());
        let rows = db_query_raw("SELECT id FROM posts".to_string());
        assert!(rows.is_ok());
        assert!(rows.unwrap().contains("p1"));
        let purged = db_delete_older_than(1_000_000_000);
        assert!(purged.is_ok());
        assert!(purged.unwrap() >= 1);
        let gone = db_query_raw("SELECT id FROM posts WHERE is_deleted = 1".to_string());
        assert!(gone.unwrap().contains("p1"));
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_backup_bad_path() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = format!(
            "{}/soshal_test_{}_badpath.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_init(path.clone()).is_ok());
        let bad = format!(
            "{}/no_such_dir_soshal_xyz/backup.db",
            std::env::temp_dir().to_string_lossy()
        );
        let res = db_backup(bad);
        assert!(res.is_err());
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
    }
}
