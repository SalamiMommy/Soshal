//! Database schema migrations.

pub mod migrations;

use crate::block_on;
use crate::libsql::{params, Connection};
use migrations::{
    v1_create_tables, v2_group_channels, v3_group_thread_reactions, v4_group_password,
    v5_performance_indexes, v6_index_cleanup, v7_query_optimizations, v8_index_cleanup,
};

/// Latest schema version the migration runner produces.
pub const SCHEMA_VERSION: i64 = 8;

/// Columns added by ALTER TABLE in the pre-squash migrations v008-v013 but
/// lost when they were collapsed into v001_initial. Legacy databases created
/// before the squash lack them; `CREATE TABLE IF NOT EXISTS` silently skips
/// existing tables, so v001's index batch fails unless the columns are healed
/// first. Mirrors the dropped ALTER statements exactly.
const LEGACY_ALTER_COLUMNS: &[(&str, &str, &str)] = &[
    ("posts", "rsvp_event_id", "TEXT"),
    ("posts", "category", "TEXT"),
    ("posts", "reposts_count", "INTEGER NOT NULL DEFAULT 0"),
    ("posts", "event_lat", "REAL"),
    ("posts", "event_lng", "REAL"),
    ("reminders", "trigger_at", "INTEGER"),
    ("users", "follower_count", "INTEGER NOT NULL DEFAULT 0"),
    ("escrows", "buyer_confirmed", "INTEGER NOT NULL DEFAULT 0"),
    ("escrows", "seller_confirmed", "INTEGER NOT NULL DEFAULT 0"),
];

/// Adds columns lost in the migration squash to legacy databases. Runs before
/// the version short-circuit so both empty-`_migrations` databases and
/// pre-squash databases (versions 1-13) converge on the canonical schema.
/// Idempotent: a column is added only when its table exists and the column is
/// missing.
fn heal_legacy_schema(conn: &Connection) -> Result<(), crate::error::DbError> {
    for &(table, column, ddl) in LEGACY_ALTER_COLUMNS {
        let table_exists: bool = crate::query::query_first(
            conn,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![table],
            |r| r.get::<i64>(0),
        )?
        .unwrap_or(0)
            > 0;
        if !table_exists {
            continue;
        }
        let has_column: bool = crate::query::query_first(
            conn,
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
            params![table, column],
            |r| r.get::<i64>(0),
        )?
        .unwrap_or(0)
            > 0;
        if has_column {
            continue;
        }
        block_on(conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
            (),
        ))?;
    }

    // v006 orphan purge (lost in the squash): legacy search_index.rs assigned
    // auto rowids, leaving posts_fts rows with no backing posts row that
    // collided with the posts_ai trigger's explicit rowid inserts. Only
    // positive rowids without a posts row are removed: posts-backed rows
    // match posts.rowid, and post-fix profile rows live in negative rowid
    // space. Idempotent; runs only when both tables exist.
    let purge_ready: bool = crate::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('posts', 'posts_fts')",
        (),
        |r| r.get::<i64>(0),
    )?
    .unwrap_or(0)
        == 2;
    if purge_ready {
        block_on(conn.execute_batch(
            "DELETE FROM posts_fts
             WHERE rowid > 0
               AND rowid NOT IN (SELECT rowid FROM posts);",
        ))?;
    }
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<(), crate::error::DbError> {
    block_on(conn.execute_batch("CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')));"))?;

    heal_legacy_schema(conn)?;

    let current: i64 = block_on(async {
        let mut rows = conn
            .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
            .await?;
        if let Some(row) = rows.next().await? {
            Ok::<i64, crate::error::DbError>(row.get::<i64>(0)?)
        } else {
            Ok(0)
        }
    })?;

    if current >= SCHEMA_VERSION {
        return Ok(());
    }

    block_on(conn.execute_batch("BEGIN IMMEDIATE"))?;

    type StepFn = fn(&Connection) -> Result<(), crate::error::DbError>;
    let steps: &[(i64, StepFn)] = &[
        (1, |c| v1_create_tables(c).map_err(Into::into)),
        (2, |c| v2_group_channels(c).map_err(Into::into)),
        (3, |c| v3_group_thread_reactions(c).map_err(Into::into)),
        (4, |c| v4_group_password(c).map_err(Into::into)),
        (5, |c| v5_performance_indexes(c).map_err(Into::into)),
        (6, |c| v6_index_cleanup(c).map_err(Into::into)),
        (7, |c| v7_query_optimizations(c).map_err(Into::into)),
        (8, |c| v8_index_cleanup(c).map_err(Into::into)),
    ];

    for &(version, step_fn) in steps {
        if version > current {
            if let Err(e) = step_fn(conn) {
                let _ = block_on(conn.execute_batch("ROLLBACK"));
                return Err(e);
            }
        }
    }

    if let Err(e) = block_on(conn.execute_batch("COMMIT")) {
        let _ = block_on(conn.execute_batch("ROLLBACK"));
        return Err(e.into());
    }
    Ok(())
}
