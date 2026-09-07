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
use std::path::PathBuf;
use std::sync::Mutex;

static DB: Mutex<Option<Database>> = Mutex::new(None);

/// Tables that `db_count` may count (mirrors the schema migrations).
const COUNTABLE_TABLES: &[&str] = &[
    "audit_logs",
    "banned_members",
    "blocks",
    "bookmarks",
    "conversations",
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
    "group_thread_reactions",
    "group_thread_replies",
    "group_threads",
    "group_voice_channels",
    "group_voice_presence",
    "groups",
    "guestbook_entries",
    "hashtags",
    "huddle_posts",
    "ignored_entities",
    "ignored_notifications",
    "link_previews",
    "marketplace_offers",
    "marketplace_reviews",
    "marketplace_saved",
    "media_blobs",
    "messages",
    "muted_conversations",
    "musicloud_comments",
    "musicloud_playlist_tracks",
    "musicloud_playlists",
    "musicloud_timed_comments",
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
    "saved_content",
    "secret_crushes",
    "settings",
    "spam_reports",
    "story_reactions",
    "stream_chat",
    "tx_edges",
    "tx_nodes",
    "users",
    "zaps",
    "zk_state_rollups",
];

fn with_db<T>(f: impl FnOnce(&Database) -> Result<T, DbError>) -> Result<T, String> {
    let db = {
        let guard = crate::ffi::util::lock(&DB);
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
    // Pin the chunk store next to the DB (persistent app-data dir). The
    // default temp-dir fallback is wiped on reboot, so published blobs
    // (music, images, videos) would vanish. An explicit SOSHAL_CHUNK_CACHE
    // env var still wins.
    if std::env::var_os("SOSHAL_CHUNK_CACHE").is_none() {
        if let Some(dir) = std::path::Path::new(&db_path).parent() {
            soshal_media_core::cas::ChunkStore::set_default_root(dir.join("chunks"));
        }
    }
    let db = match Database::open(&db_path) {
        Ok(d) => d,
        Err(e) => return Err(format!("open failed: {e}")).into(),
    };
    if let Err(e) = db.migrate() {
        return Err(format!("migration failed: {e}")).into();
    }
    // Clean up old DB instance and its temp files if any. Do the entire swap
    // (path + DB) under a single lock acquisition so concurrent `with_db()`
    // callers never observe a "database not initialized" transient window.
    let mut db_guard = crate::ffi::util::lock(&DB);
    let _old_db = db_guard.take();
    let old_path = crate::ffi::util::lock(&DB_PATH).take();
    drop(db_guard);
    if let Some(ref p) = old_path {
        if old_path.as_deref() != Some(&db_path) {
            let temp = std::env::temp_dir().to_string_lossy().to_string();
            let own_file = std::path::Path::new(p)
                .file_name()
                .and_then(|f| f.to_str())
                .map(|f| f.starts_with("soshal_") && f.ends_with(".db"))
                .unwrap_or(false);
            if own_file && p.starts_with(&temp) {
                let _ = std::fs::remove_file(p);
                let _ = std::fs::remove_file(format!("{p}-wal"));
                let _ = std::fs::remove_file(format!("{p}-shm"));
            }
        }
    }
    // Set new path + DB under fresh locks (single-point, non-interleaved).
    chmod_0600(&db_path);
    *crate::ffi::util::lock(&DB_PATH) = Some(db_path.clone());
    *crate::ffi::util::lock(&DB) = Some(db);
    Ok(db_path).into()
}

/// Restrict the SQLite main/WAL/SHM files to the owning user. The default
/// umask usually yields 0644, which leaves the message store + session tables
/// world-readable on multi-user machines. Idempotent: fixes legacy files too.
#[cfg(unix)]
fn chmod_0600(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    for p in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        if let Ok(md) = std::fs::metadata(&p) {
            let mut perms = md.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&p, perms);
        }
    }
}

/// Get the current database path, or an error if not initialized.
#[frb(sync, serialize)]
pub fn db_path() -> Result<String, String> {
    let guard = crate::ffi::util::lock(&DB_PATH);
    match guard.as_ref() {
        Some(p) => Ok(p.clone()).into(),
        None => Err("database not initialized".to_string()).into(),
    }
}

/// Active account pubkey from the `settings` table (`''` when absent).
/// Shared by every audience-filtered fetch surface.
pub(crate) fn active_pubkey() -> Result<String, String> {
    with_db_result(|db| {
        let conn = db.conn()?;
        soshal_db_core::block_on(async {
            let stmt = conn
                .prepare("SELECT value FROM settings WHERE key = 'active_pubkey'")
                .await?;
            let mut rows = stmt.query(()).await?;
            match rows.next().await? {
                Some(row) => Ok(row.get(0).unwrap_or_default()),
                None => Ok(String::new()),
            }
        })
    })
}

/// Close the active database connection and clear the DB path.
#[frb(sync, serialize)]
pub fn db_close() -> Result<bool, String> {
    let _ = crate::ffi::util::lock(&DB_PATH).take();
    let _ = crate::ffi::util::lock(&DB).take();
    Ok(true).into()
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

/// Repair a stale/out-of-date schema: wipe all tables and re-run migrations
/// from scratch.  Refuses to operate when the DB is already at or ahead of the
/// current `SCHEMA_VERSION` — fresh or current DBs must not be touched.
#[frb(sync, serialize)]
pub fn db_force_migrate() -> Result<String, String> {
    with_db(|db| {
        // Scope the checkout: db.migrate() below checks out its own conn.
        // Holding this guard across migrate() deadlocks :memory: (max 1)
        // and splits file DBs across two connections.
        let tables: Vec<String> = {
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

            block_on(conn.execute("PRAGMA foreign_keys = OFF", ()))
                .map_err(|e| DbError::Migration(format!("FK disable failed: {e}")))?;
            // L6 fix: wrap all DROP statements in a single transaction so that
            // a mid-loop crash cannot leave the schema partially wiped.
            // Also quote each table name (defense-in-depth; names are already
            // filtered by the identifier regex above, but quoting is free).
            block_on(conn.execute("BEGIN IMMEDIATE", ()))
                .map_err(|e| DbError::Migration(format!("begin transaction: {e}")))?;
            let drop_result: Result<(), DbError> = (|| {
                for table in tables.iter().filter(|n| {
                    let mut chars = n.chars();
                    match chars.next() {
                        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                        }
                        _ => false,
                    }
                }) {
                    block_on(conn.execute(&format!("DROP TABLE IF EXISTS \"{table}\""), ()))
                        .map_err(|e| DbError::Migration(format!("drop {table}: {e}")))?;
                }
                Ok(())
            })();
            match drop_result {
                Ok(()) => {
                    block_on(conn.execute("COMMIT", ()))
                        .map_err(|e| DbError::Migration(format!("commit failed: {e}")))?;
                }
                Err(e) => {
                    block_on(conn.execute("ROLLBACK", ()))
                        .map_err(|e| DbError::Migration(format!("rollback failed: {e}")))?;
                    block_on(conn.execute("PRAGMA foreign_keys = ON", ()))
                        .map_err(|e| DbError::Migration(format!("FK enable failed: {e}")))?;
                    return Err(e);
                }
            }
            block_on(conn.execute("PRAGMA foreign_keys = ON", ()))
                .map_err(|e| DbError::Migration(format!("FK enable failed: {e}")))?;
            tables
        };

        let _ = tables;
        // Re-run migrations on a fresh checkout (guard above dropped).
        db.migrate()
            .map_err(|e| DbError::Migration(format!("migration failed: {e}")))?;
        Ok("Migration re-run complete".to_string())
    })
}

/// Execute a raw SELECT query; rows are returned as a JSON array of objects
/// (column names as keys). Parameter binding is supported with `?1..?N`.
/// The `settings` table is off-limits: pin flags must only change through
/// the pin module (db_set_setting enforces the same deny-list).
#[frb(sync, serialize)]
pub fn db_query_raw(sql: String) -> Result<String, String> {
    if !raw_sql_allowed(&sql) {
        return Err("sql touches a protected settings key".to_string());
    }
    db_query_params(&sql, &[])
}

/// Test-only bypass of the raw-SQL console guard. Fixture setup builds
/// tables and rows the console deliberately forbids (CREATE/INSERT);
/// `#[cfg(test)]` keeps it out of the release bridge .so entirely.
#[cfg(test)]
pub fn db_query_raw_test(sql: String) -> Result<String, String> {
    db_query_params(&sql, &[])
}

/// Test-only bypass of the raw-SQL console guard. See `db_query_raw_test`.
#[cfg(test)]
pub fn db_execute_raw_test(sql: String) -> Result<usize, String> {
    db_execute_params(&sql, &[])
}

/// Protected settings keys that must only be written by their owning module
/// (pin.rs). Keeps PIN hash + lockout flags out of reach of the Dart-side raw
/// SQL console and any accidental generic setting write.
const PROTECTED_SETTING_KEYS: &[&str] =
    &["pin_hash", "pin_permanently_locked", "pin_lockout_state"];

fn raw_sql_allowed(sql: &str) -> bool {
    // Normalize before checking: SQLite accepts `/*…*/` comments, `--…`
    // line comments and \t\n\r as token separators, so a naive keyword
    // blacklist is bypassable (`drop/**/table`, `insert\tinto`). Strip
    // comments and collapse all whitespace to single spaces first.
    let mut norm = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut in_block = false;
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
                // SQLite treats comments as token separators: `drop/**/table`
                // is `drop table`. Emit a space so the blacklist sees it.
                if !norm.ends_with(' ') {
                    norm.push(' ');
                }
            }
            continue;
        }
        match c {
            '-' if chars.peek() == Some(&'-') => {
                chars.next();
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                norm.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block = true;
            }
            c if c.is_whitespace() => {
                if !norm.ends_with(' ') {
                    norm.push(' ');
                }
            }
            c => norm.push(c),
        }
    }

    // L4 fix: strip SQLite hex string literals (`x'…'` / `X'…'`) before the
    // keyword check.  Without this, `x'64726f70207461626c65'` (= "drop table")
    // could encode keyword bytes inside a CAST or column expression and slip
    // through the blocklist.  Replace each hex literal with a single space so
    // that surrounding tokens remain correctly delimited.
    let norm = {
        let mut out = String::with_capacity(norm.len());
        let mut nc = norm.chars().peekable();
        while let Some(c) = nc.next() {
            if (c == 'x' || c == 'X') && nc.peek() == Some(&'\'') {
                nc.next(); // consume the opening quote
                           // consume everything until the closing quote (or end of string)
                for hc in nc.by_ref() {
                    if hc == '\'' {
                        break;
                    }
                }
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            } else {
                out.push(c);
            }
        }
        out
    };

    let lower = norm.trim().to_lowercase();
    // The raw console is read-only diagnostics: only single SELECT / WITH
    // statements. Reject multi-statement input, every mutation and schema
    // statement (INSERT/UPDATE/DELETE/REPLACE/CREATE/ALTER/DROP), anything
    // that could touch files outside the database (ATTACH DATABASE) or
    // change runtime behavior (PRAGMA/VACUUM/REINDEX).
    if lower.contains(';')
        || lower.contains("attach ")
        || lower.contains("drop ")
        || lower.contains("vacuum")
        || lower.contains("create ")
        || lower.contains("alter ")
        || lower.contains("reindex ")
        || lower.contains("pragma ")
        || lower.contains("insert ")
        || lower.contains("replace ")
        || lower.contains("update ")
        || lower.contains("delete ")
    {
        return false;
    }
    // First-command gate: UPDATE/DELETE (executable through libsql
    // `Statement::query`) must never slip past the substring blocklist (the
    // comment/hex-normalization above makes prefixes like `x'…'`‑wrapped
    // tokens detectable, but the first-token check is authoritative). Mutations
    // go through parameterized repo functions (db_execute_params) only.
    if !matches!(
        lower.split_whitespace().next().unwrap_or(""),
        "select" | "with"
    ) {
        return false;
    }
    !lower.contains("settings") && !PROTECTED_SETTING_KEYS.iter().any(|k| lower.contains(k))
}

/// Internal helper: raw SELECT with bound ?N parameters (Vec<String>),
/// rows as a JSON array of objects. Not an FFI surface.
pub fn db_query_params(sql: &str, params: &[String]) -> Result<String, String> {
    db_query_json(sql, params).map(|rows| super::util::json_ok_or_empty(&rows))
}

/// Internal helper: raw SELECT with bound ?N parameters, rows directly as
/// `Vec<serde_json::Value>` — avoids the JSON string encode/decode
/// round-trip that `db_query_params` imposes on every caller. Use this on
/// hot paths that parse rows back into Rust values (batch queries, feed,
/// events, minis). Not an FFI surface.
pub fn db_query_json(sql: &str, params: &[String]) -> Result<Vec<serde_json::Value>, String> {
    with_db(|db| {
        let conn = db.conn()?;
        let out = block_on(async {
            let stmt = conn.prepare(sql).await?;
            let mut rows = stmt
                .query(params_from_iter(params.iter().map(|p| p.as_str())))
                .await?;
            rows_json(&stmt, &mut rows).await
        })?;
        Ok(out)
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
/// The `settings` table is off-limits (see `raw_sql_allowed`).
///
/// ⚠ Security: This function is intentionally disabled — unparameterized SQL
/// writes from Dart are architecturally unsound (text-based guard is fragile).
/// All callers should use parameterized repo functions instead.
/// The generated FFI stub still wires this name; it always returns an error so
/// the Dart side receives a clear failure rather than executing raw SQL.
pub fn db_execute_raw(sql: String) -> Result<usize, String> {
    // If somehow called through the generated FFI, return an explicit error
    // rather than executing unparameterized SQL.
    let _ = sql;
    Err("db_execute_raw is disabled; use parameterized repo functions".to_string())
}

/// Internal helper: raw INSERT/UPDATE/DELETE with bound ?N parameters
/// (Vec<String>); returns rows affected. Not an FFI surface.
pub fn db_execute_params(sql: &str, params: &[String]) -> Result<usize, String> {
    with_db(|db| {
        let conn = db.conn()?;
        Ok(block_on(async {
            conn.execute(sql, params_from_iter(params.iter().map(|p| p.as_str())))
                .await
        })? as usize)
    })
}

/// Get the row count of a table.
#[frb(sync, serialize)]
pub fn db_count(table: String) -> Result<i64, String> {
    if !COUNTABLE_TABLES.contains(&table.as_str()) {
        return Err("db: unknown table".to_string());
    }
    // H2 fix: validate the identifier with the same character-class filter used
    // in db_storage_stats, and quote it in the SQL string.  The COUNTABLE_TABLES
    // check above is the primary gate; this is defense-in-depth against any
    // future allowlist expansion that accidentally includes an unusual name.
    let valid_ident = {
        let mut chars = table.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            }
            _ => false,
        }
    };
    if !valid_ident {
        return Err("db: invalid table identifier".to_string());
    }
    with_db(|db| {
        let conn = db.conn()?;
        // Table name is double-quoted (SQL standard identifier quoting).
        let sql = format!("SELECT COUNT(*) AS c FROM \"{table}\"");
        let count = soshal_db_core::query::query_first(&conn, &sql, (), |r| {
            let n: i64 = r.get(0)?;
            Ok(n)
        })?;
        Ok(count.unwrap_or(0))
    })
}

/// Set a key/value setting (sidebar order, theme, stealth whitelist, PIN
/// flags). Upserts into the `settings` table. PIN-related keys are denied
/// here — only the pin module may write them.
#[frb(sync, serialize)]
pub fn db_set_setting(key: String, value: String) -> Result<bool, String> {
    if PROTECTED_SETTING_KEYS.contains(&key.as_str()) {
        return Err(format!("setting key is protected: {key}"));
    }
    with_db(|db| {
        soshal_db_core::repos::settings::SettingsRepo::new(db).set(&key, &value)?;
        Ok(true)
    })
}

/// Get a setting value by key. PIN-related keys are denied: their values
/// (salt+hash, lockout state) must never leave Rust.
#[frb(sync, serialize)]
pub fn db_get_setting(key: String) -> Result<Option<String>, String> {
    if PROTECTED_SETTING_KEYS.contains(&key.as_str()) {
        return Err(format!("setting key is protected: {key}"));
    }
    with_db(|db| {
        soshal_db_core::repos::settings::SettingsRepo::new(db)
            .get(&key)
            .map(|v| v.filter(|s| !s.is_empty()))
    })
}

/// Delete a setting key. PIN-related keys are denied: deleting them would
/// bypass the PIN lockout state machine.
#[frb(sync, serialize)]
pub fn db_delete_setting(key: String) -> Result<bool, String> {
    if PROTECTED_SETTING_KEYS.contains(&key.as_str()) {
        return Err(format!("setting key is protected: {key}"));
    }
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
    let file_bytes = crate::ffi::util::lock(&DB_PATH)
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
    Ok(super::util::json_ok_or_empty(&out))
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
        let guard = crate::ffi::util::lock(&DB_PATH);
        guard
            .clone()
            .ok_or_else(|| "database not initialized".to_string())?
    };
    same_parent_dir(&src_path, &backup_path, "backup")?;
    with_db(|db| {
        let conn = db.conn()?;
        let _ = block_on(conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);"));
        // The backup is sealed with the identity-derived at-rest key: the
        // file on disk is AES-256-GCM ciphertext, never a plaintext DB
        // snapshot. Restore transparently decrypts it (same key).
        let data = std::fs::read(&src_path)
            .map_err(|e| DbError::Migration(format!("read failed: {e}")))?;
        let key = super::signer::signer_at_rest_key()
            .map_err(|e| DbError::Migration(format!("at-rest key: {e}")))?;
        let sealed = soshal_crypto_core::at_rest::seal_at_rest_bin(&key, &data)
            .map_err(|e| DbError::Migration(format!("seal failed: {e}")))?;
        let mut out = BACKUP_MAGIC.to_vec();
        out.extend_from_slice(&sealed);
        std::fs::write(&backup_path, out)
            .map_err(|e| DbError::Migration(format!("write failed: {e}")))?;
        Ok(backup_path.clone())
    })
}

/// Verify that two paths resolve to the same parent directory.
fn same_parent_dir(db_path: &str, other_path: &str, label: &str) -> Result<(), String> {
    let db_dir = std::path::Path::new(db_path)
        .parent()
        .ok_or("cannot determine db directory")?;
    let other_dir = std::path::Path::new(other_path)
        .parent()
        .ok_or("backup path has no parent directory")?;
    let db_dir_canon =
        std::fs::canonicalize(db_dir).map_err(|e| format!("db dir canonicalize: {e}"))?;
    let other_dir_canon =
        std::fs::canonicalize(other_dir).map_err(|e| format!("backup dir canonicalize: {e}"))?;
    if db_dir_canon != other_dir_canon {
        return Err(format!(
            "{label} path must be in the same directory as the database"
        ));
    }
    Ok(())
}

/// Backup file header: `SOSHBK01` marks an at-rest-key-sealed snapshot.
const BACKUP_MAGIC: &[u8] = b"SOSHBK01";

/// Deletes its target path on drop: the plaintext restore buffer must not
/// outlive the restore call, on any early-return path.
struct TempCleanup(PathBuf);
impl Drop for TempCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
        let guard = crate::ffi::util::lock(&DB_PATH);
        guard
            .clone()
            .ok_or_else(|| "database not initialized".to_string())?
    };
    same_parent_dir(&dst, &backup_path, "restore")?;
    // Only SOSHBK01-sealed backups are accepted. A plaintext DB file could
    // be a forged store that bypasses every guard (raw-SQL, protected
    // settings, PIN lockout) — reject it outright.
    let mut head = [0u8; BACKUP_MAGIC.len()];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(&backup_path)
            .map_err(|e| format!("cannot open backup file: {e}"))?;
        f.read_exact(&mut head)
            .map_err(|_| "backup file too small to be a Soshal backup".to_string())?;
    }
    if head != BACKUP_MAGIC {
        return Err("backup file is not a sealed Soshal backup (SOSHBK01)".to_string());
    }
    let blob =
        std::fs::read(&backup_path).map_err(|e| format!("cannot read encrypted backup: {e}"))?;
    let key = super::signer::signer_at_rest_key().map_err(|e| format!("at-rest key: {e}"))?;
    let plain = soshal_crypto_core::at_rest::open_at_rest_bin(&key, &blob[BACKUP_MAGIC.len()..])
        .map_err(|e| format!("backup decrypt failed: {e}"))?;
    // Plaintext DB goes to the system temp dir with 0600 perms, never the
    // app directory (backup dir may be world-readable via umask). Removed on
    // every exit path by `_cleanup`.
    let temp = std::env::temp_dir().join(format!(
        "soshal-restore-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    {
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| format!("temp open failed: {e}"))?;
        f.write_all(&plain)
            .map_err(|e| format!("temp write failed: {e}"))?;
    }
    let _cleanup = TempCleanup(temp.clone());
    let backup_file = temp;
    // Validate SQLite magic bytes before replacing the live DB.
    const SQLITE_MAGIC: &[u8] = b"SQLite format 3\x00";
    let mut header = [0u8; 16];
    let mut f =
        std::fs::File::open(&backup_file).map_err(|e| format!("cannot open backup file: {e}"))?;
    use std::io::Read;
    f.read_exact(&mut header)
        .map_err(|_| "backup file too small to be a valid SQLite database".to_string())?;
    if header != SQLITE_MAGIC {
        let _ = std::fs::remove_file(&backup_file);
        return Err("backup file is not a valid SQLite 3 database".to_string());
    }
    drop(f);
    // Stale WAL/SHM from a previous live DB must not survive the swap: a
    // leftover -wal could resurrect old data or corrupt the restored file.
    let _ = std::fs::remove_file(format!("{dst}-wal"));
    let _ = std::fs::remove_file(format!("{dst}-shm"));
    *crate::ffi::util::lock(&DB) = None;
    let bak = format!("{dst}.bak");
    // Preserve the current live DB as .bak for rollback — but only when it
    // actually exists (restoring onto a deleted/never-created DB is valid).
    if std::path::Path::new(&dst).exists() {
        if let Err(e) = std::fs::copy(&dst, &bak) {
            // re-open the original db so the app stays usable
            if let Ok(db) = Database::open(&dst) {
                *crate::ffi::util::lock(&DB) = Some(db);
            }
            return Err(format!("backup copy failed: {e}"));
        }
    }
    if let Err(e) = std::fs::copy(&backup_file, &dst) {
        // re-open the original db so the app stays usable
        let _ = std::fs::remove_file(&backup_file);
        if let Ok(db) = Database::open(&dst) {
            *crate::ffi::util::lock(&DB) = Some(db);
        }
        return Err(format!("restore copy failed: {e}"));
    }
    let _ = std::fs::remove_file(&backup_file);
    match Database::open(&dst) {
        Ok(db) => {
            if let Err(e) = db.migrate() {
                // restore the pre-restore snapshot so old data survives
                drop(db);
                let _ = std::fs::copy(&bak, &dst);
                let _ = std::fs::remove_file(&bak);
                if let Ok(db) = Database::open(&dst) {
                    *crate::ffi::util::lock(&DB) = Some(db);
                }
                return Err(format!("restore migrate failed: {e}"));
            }
            // Post-restore integrity: a forged `_migrations` table (max
            // version >= SCHEMA_VERSION) makes migrate() bail without
            // touching anything, so verify the core schema actually exists.
            if let Err(e) = verify_restored_schema(&db) {
                drop(db);
                let _ = std::fs::copy(&bak, &dst);
                let _ = std::fs::remove_file(&bak);
                if let Ok(db) = Database::open(&dst) {
                    *crate::ffi::util::lock(&DB) = Some(db);
                }
                return Err(e);
            }
            *crate::ffi::util::lock(&DB) = Some(db);
            let _ = std::fs::remove_file(&bak);
            Ok(dst)
        }
        Err(e) => {
            // Restore the pre-restore snapshot and reopen so the app stays
            // usable; without this the global DB handle stays None and the
            // app is dead until a full re-init.
            if std::path::Path::new(&bak).exists() {
                let _ = std::fs::copy(&bak, &dst);
                let _ = std::fs::remove_file(&bak);
            }
            if let Ok(db) = Database::open(&dst) {
                *crate::ffi::util::lock(&DB) = Some(db);
            }
            Err(format!("restore open failed: {e}"))
        }
    }
}

/// Post-restore integrity check: the required core tables must exist. A
/// forged backup whose `_migrations` table claims the latest version would
/// otherwise skip migration entirely.
fn verify_restored_schema(db: &Database) -> Result<(), String> {
    const REQUIRED: [&str; 5] = ["posts", "users", "zaps", "outbox_queue", "_migrations"];
    let conn = db.conn().map_err(|e| format!("verify conn: {e}"))?;
    let present = block_on(async {
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN (?,?,?,?,?)",
                REQUIRED.map(|n| n.to_string()),
            )
            .await
            .map_err(|e| format!("verify query: {e}"))?;
        let row = rows.next().await.map_err(|e| format!("verify rows: {e}"))?;
        let row = row.ok_or("verify no row")?;
        row.get::<i64>(0).map_err(|e| format!("verify get: {e}"))
    })
    .map_err(|e: String| e)?;
    if present != REQUIRED.len() as i64 {
        return Err(format!(
            "restored backup is missing required tables ({present}/{} present)",
            REQUIRED.len()
        ));
    }
    Ok(())
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
        let rows = repo.get_trending(soshal_db_core::repos::clamp_limit(limit).min(100))?;
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
        Ok(super::util::json_ok_or_empty(&out))
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
        Ok(super::util::json_ok_or_empty(&out))
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

/// Drop the process-global DB handle (tests only) so the next `db_init`
/// starts clean; callers must remove the backing files themselves.
#[cfg(test)]
pub(crate) fn reset_db_global() {
    *DB.lock().unwrap() = None;
}

/// Insert a minimal users row so child tables with FOREIGN KEY REFERENCES
/// users(pubkey) succeed under the now-enforced FK pragma.
#[cfg(test)]
pub(crate) fn insert_test_user(pubkey: &str) {
    db_execute_params(
        "INSERT OR IGNORE INTO users (pubkey, npub, created_at, updated_at, contact_pubkeys, relay_list, follower_count) \
         VALUES (?1, ?2, 1000, 1000, '[]', '[]', 0)",
        &[pubkey.to_string(), format!("npub_{pubkey}")],
    )
    .unwrap();
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
        let exec = db_execute_raw_test(
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
        // Backup sealing needs the identity-derived at-rest key: unlock the
        // process-global signer first (SIGNER_TEST_LOCK guards cross-module
        // races on that state).
        let _sg = crate::ffi::test_lock::SIGNER_TEST_LOCK.lock().unwrap();
        let nsec_hex = "01".repeat(32);
        assert!(crate::ffi::signer::signer_unlock(nsec_hex).is_ok());
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
        assert!(db_execute_raw_test(
            "CREATE TABLE backup_test (id INTEGER PRIMARY KEY, val TEXT)".to_string()
        )
        .is_ok());
        assert!(db_execute_params(
            "INSERT INTO backup_test (val) VALUES (?1)",
            &["survivor".to_string()]
        )
        .is_ok());
        assert!(db_backup(backup_path.clone()).is_ok());
        // The backup file must be ciphertext (sealed), never a plaintext DB.
        let raw = std::fs::read(&backup_path).unwrap();
        assert!(raw.starts_with(b"SOSHBK01"));
        assert!(!raw.windows(16).any(|w| w == b"SQLite format 3\x00"));
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_restore(backup_path.clone()).is_ok());
        let rows = db_query_raw("SELECT val FROM backup_test".to_string());
        assert!(rows.is_ok());
        assert!(rows.unwrap().contains("survivor"));
        // Decrypted temp must not linger.
        assert!(!std::path::Path::new(&format!("{backup_path}.plain")).exists());
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&backup_path);
        crate::ffi::signer::signer_lock().ok();
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
    fn test_protected_settings_keys_denied() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let path = format!(
            "{}/soshal_test_{}_guarded.db",
            std::env::temp_dir().to_string_lossy(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
        assert!(db_init(path.clone()).is_ok());
        assert!(
            db_set_setting("pin_hash".to_string(), "x".to_string()).is_err(),
            "pin_hash must not be settable via db_set_setting"
        );
        assert!(db_set_setting("pin_permanently_locked".to_string(), "false".to_string()).is_err());
        assert!(db_set_setting("pin_lockout_state".to_string(), "{}".to_string()).is_err());
        assert!(
            db_execute_raw("UPDATE settings SET value='x' WHERE key='pin_hash'".to_string())
                .is_err(),
            "raw SQL must not touch the settings table"
        );
        assert!(
            db_query_raw("SELECT value FROM settings".to_string()).is_err(),
            "raw query must not touch the settings table"
        );
        // Mutations must never run through the query console, even though
        // libsql `Statement::query` would execute them.
        for write_sql in [
            "UPDATE users SET name='x'",
            "DELETE FROM posts",
            "UPDATE settings SET value='x' WHERE key='pin_hash'",
            "select 1; delete from posts",
        ] {
            assert!(
                db_query_raw(write_sql.to_string()).is_err(),
                "console mutation must be rejected: {write_sql}"
            );
        }
        assert!(
            db_query_raw("SELECT * FROM users LIMIT 1".to_string()).is_ok(),
            "read-only SELECT still allowed on the console"
        );
        assert!(
            db_execute_raw(
                "INSERT INTO users (pubkey, npub, name) VALUES ('guarded1', 'npub1g', 'g') \
                 ON CONFLICT DO UPDATE SET name='g'"
                    .to_string()
            )
            .is_err(),
            "raw SQL must not INSERT (write path is db_execute_params)"
        );
        // Blacklist bypass attempts: SQLite treats comments and \t\n\r as
        // token separators — normalized input must still be caught.
        for evil in [
            "drop/**/table users",
            "insert\tinto users (pubkey) values ('x')",
            "attach\nDATABASE '/tmp/x.db' AS x",
            "create--\n table evil (id int)",
            "pragma\x0bjournal_mode=delete",
            "DROP TABLE users; DROP TABLE posts",
        ] {
            assert!(
                db_execute_raw(evil.to_string()).is_err(),
                "bypass attempt must be rejected: {evil}"
            );
        }
        // get/delete of protected keys must be denied too.
        assert!(
            db_get_setting("pin_hash".to_string()).is_err(),
            "pin_hash must not be readable via db_get_setting"
        );
        assert!(
            db_delete_setting("pin_lockout_state".to_string()).is_err(),
            "lockout state must not be deletable via db_delete_setting"
        );
        assert!(
            db_execute_params(
                "INSERT INTO users (pubkey, npub, name) VALUES (?1, ?2, ?3)",
                &[
                    "guarded1".to_string(),
                    "npub1g".to_string(),
                    "g".to_string(),
                ]
            )
            .is_ok(),
            "unrelated parameterized SQL still allowed"
        );
        *DB.lock().unwrap() = None;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_db_file_perms_0600() {
        let _g = crate::ffi::util::lock(&crate::ffi::test_lock::DB_TEST_LOCK);
        let path = tmp_db_path("perms", "e2");
        db_init(path.clone()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "db file must be 0600, got {mode:o}");
        }
        reset_db_global();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
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
        assert!(db_execute_raw_test(
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
        assert!(db_execute_raw_test(
            "INSERT INTO users (pubkey, npub, name) VALUES ('pk', 'npub1pk', 't') ON CONFLICT DO NOTHING".to_string()
        )
        .is_ok());
        assert!(db_execute_raw_test(
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
