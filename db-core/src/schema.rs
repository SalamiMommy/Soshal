//! Database schema migrations.

// NOTE: Do NOT add new migration files. The app has not been released yet —
// modify v001_initial.rs directly to flatten all schema changes. Migrations
// are only needed after the first public release when real user data exists.

pub mod migrations;

use crate::block_on;
use crate::libsql::{params, Connection};
use migrations::{
    create_dating_unmatch_actor_column, v11_index_cleanup, v12_saved_content_playlists,
    v1_create_tables, v2_group_channels, v3_group_thread_reactions, v4_group_password,
    v5_performance_indexes, v6_index_cleanup, v7_query_optimizations, v8_index_cleanup,
    v9_trigger_optimization,
};

/// Latest schema version the migration runner produces.
pub const SCHEMA_VERSION: i64 = 12;

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

    // ignored_notifications lives in v001 but pre-squash/older dev databases
    // already past v1 won't re-run it. Ensure the table exists idempotently so
    // the Ignore/Mute repo queries never hit a missing table. (Flattened into
    // v001 per the "no new migrations pre-release" convention; this heals any
    // database that predates the table.)
    block_on(conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ignored_notifications (
            pubkey TEXT NOT NULL,
            from_pubkey TEXT NOT NULL DEFAULT '',
            event_id TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (pubkey, kind, from_pubkey, event_id)
        );",
    ))?;

    // idx_users_follower_count lives in v001 but pre-squash databases skipped
    // it via the version short-circuit; the counter-ordered feed window scans
    // without it. CREATE INDEX IF NOT EXISTS is idempotent on legacy DBs, but
    // heal runs BEFORE the version steps that create `users` on a fresh DB,
    // so it is guarded on the table existing (v1 already creates the index).
    let users_table_exists: bool = crate::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'",
        (),
        |r| r.get::<i64>(0),
    )?
    .unwrap_or(0)
        > 0;
    if users_table_exists {
        block_on(conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_users_follower_count ON users(follower_count DESC);",
        ))?;
    }

    // dating_unmatches moved to a composite (actor_pubkey, pubkey) key in v10
    // (was target-only). Fresh DBs reach v10 through the version steps, but
    // legacy databases short-circuit before it (their _migrations version can
    // exceed SCHEMA_VERSION), so the rebuild must happen here too — else the
    // actor-scoped repo queries fail on `actor_pubkey`. SQLite cannot ALTER a
    // PRIMARY KEY, so the table is swapped. Runs only when the table is still
    // in the old shape (missing the column), so it is idempotent.
    let table_exists: bool = crate::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='dating_unmatches'",
        (),
        |r| r.get::<i64>(0),
    )?
    .unwrap_or(0)
        > 0;
    let needs_actor_column: bool = table_exists
        && crate::query::query_first(
            conn,
            "SELECT COUNT(*) FROM pragma_table_info('dating_unmatches') WHERE name='actor_pubkey'",
            (),
            |r| r.get::<i64>(0),
        )?
        .unwrap_or(0)
            == 0;
    if needs_actor_column {
        block_on(conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dating_unmatches_new (
                actor_pubkey TEXT NOT NULL,
                pubkey TEXT NOT NULL,
                unmatched_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (actor_pubkey, pubkey)
            );
            DROP TABLE IF EXISTS dating_unmatches;
            ALTER TABLE dating_unmatches_new RENAME TO dating_unmatches;",
        ))?;
    }
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<(), crate::error::DbError> {
    block_on(conn.execute_batch("CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')));"))?;

    // Heal + version steps run inside one BEGIN IMMEDIATE txn so a failed
    // migrate never leaves partial ALTERs committed while steps roll back.
    block_on(conn.execute_batch("BEGIN IMMEDIATE"))?;

    if let Err(e) = heal_legacy_schema(conn) {
        let _ = block_on(conn.execute_batch("ROLLBACK"));
        return Err(e);
    }

    let current: i64 = match block_on(async {
        let mut rows = conn
            .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
            .await?;
        if let Some(row) = rows.next().await? {
            Ok::<i64, crate::error::DbError>(row.get::<i64>(0)?)
        } else {
            Ok(0)
        }
    }) {
        Ok(v) => v,
        Err(e) => {
            let _ = block_on(conn.execute_batch("ROLLBACK"));
            return Err(e);
        }
    };

    if current >= SCHEMA_VERSION {
        if let Err(e) = block_on(conn.execute_batch("COMMIT")) {
            let _ = block_on(conn.execute_batch("ROLLBACK"));
            return Err(e.into());
        }
        return Ok(());
    }

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
        (9, |c| v9_trigger_optimization(c).map_err(Into::into)),
        (10, |c| {
            create_dating_unmatch_actor_column(c).map_err(Into::into)
        }),
        (11, |c| v11_index_cleanup(c).map_err(Into::into)),
        (12, |c| v12_saved_content_playlists(c).map_err(Into::into)),
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
