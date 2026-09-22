//! Database schema migrations.

// NOTE: Migrations are enabled. `v001_initial.rs` is the squashed pre-release
// baseline — it was formed by flattening the pre-public-release migration
// chain (v001-v013) into a single step, so every existing database is already
// at version 1 with the full schema. From here on, schema changes MUST land as
// new `v0NN_*.rs` migration files: register the module + re-export in
// `migrations/mod.rs`, add it to the `steps` array in `migrate()`, and bump
// `SCHEMA_VERSION`. Each migration runs inside the `BEGIN IMMEDIATE` txn and
// records its own version (`INSERT OR IGNORE INTO _migrations (version)
// VALUES (N)`); it must stay idempotent so re-runs and `migrate()` on
// already-migrated databases are no-ops.

pub mod migrations;

use crate::block_on;
use crate::libsql::{params, Connection};
use migrations::v1_create_tables;

/// Latest schema version the migration runner produces.
pub const SCHEMA_VERSION: i64 = 1;

/// Columns added by ALTER TABLE in the pre-squash migrations but
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
    ("groups", "password_hash", "TEXT"),
    ("group_messages", "room_id", "TEXT NOT NULL DEFAULT ''"),
    ("musiclouds", "blob_hash", "TEXT NOT NULL DEFAULT ''"),
    ("musiclouds", "media_size", "INTEGER NOT NULL DEFAULT 0"),
    ("musiclouds", "hashtags", "TEXT NOT NULL DEFAULT '[]'"),
    ("musiclouds", "d", "TEXT NOT NULL DEFAULT ''"),
];

/// Adds columns and tables lost in the migration squash to legacy databases.
/// Runs before the version short-circuit so both empty-`_migrations` databases
/// and pre-squash databases (versions 1-13) converge on the canonical schema.
/// Idempotent: columns and tables are only added when missing.
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

    // Ensure tables from earlier migrations exist idempotently for legacy DBs.
    block_on(conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ignored_notifications (
            pubkey TEXT NOT NULL,
            from_pubkey TEXT NOT NULL DEFAULT '',
            event_id TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (pubkey, kind, from_pubkey, event_id)
        );
        CREATE TABLE IF NOT EXISTS musicloud_playlists (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL,
            title TEXT NOT NULL,
            is_private INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS musicloud_timed_comments (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL,
            pubkey TEXT NOT NULL,
            timestamp_ms INTEGER NOT NULL DEFAULT 0,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS group_rooms (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL DEFAULT '',
            topic TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            color TEXT NOT NULL DEFAULT '#8b5cf6',
            position INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS group_threads (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            reply_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS group_thread_replies (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL DEFAULT '',
            parent_id TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS group_voice_channels (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL DEFAULT '',
            position INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS group_voice_presence (
            channel_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            joined_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (channel_id, pubkey)
        );
        CREATE TABLE IF NOT EXISTS group_thread_reactions (
            thread_id TEXT NOT NULL DEFAULT '',
            reply_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (thread_id, reply_id, pubkey, emoji)
        );
        CREATE TABLE IF NOT EXISTS group_room_reactions (
            group_id TEXT NOT NULL DEFAULT '',
            room_id TEXT NOT NULL DEFAULT '',
            message_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (message_id, pubkey, emoji)
        );
        CREATE TABLE IF NOT EXISTS zk_state_rollups (
            thread_id TEXT PRIMARY KEY,
            genesis_root TEXT NOT NULL,
            final_state_root TEXT NOT NULL,
            operation_count INTEGER NOT NULL,
            verified_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS saved_content (
            kind INTEGER NOT NULL,
            id TEXT NOT NULL,
            pubkey TEXT NOT NULL DEFAULT '',
            d TEXT NOT NULL DEFAULT '',
            media_type TEXT NOT NULL DEFAULT '',
            media_url TEXT NOT NULL DEFAULT '',
            text_overlay TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            thumbnail TEXT NOT NULL DEFAULT '',
            blob_hash TEXT NOT NULL DEFAULT '',
            media_size INTEGER NOT NULL DEFAULT 0,
            audience TEXT NOT NULL DEFAULT 'public',
            hashtags TEXT NOT NULL DEFAULT '[]',
            host_ready INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0,
            saved_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (kind, id)
        );
        CREATE TABLE IF NOT EXISTS musicloud_playlist_tracks (
            playlist_id TEXT NOT NULL,
            track_id TEXT NOT NULL,
            pubkey TEXT NOT NULL DEFAULT '',
            d TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            thumbnail TEXT NOT NULL DEFAULT '',
            audio_url TEXT NOT NULL DEFAULT '',
            blob_hash TEXT NOT NULL DEFAULT '',
            media_size INTEGER NOT NULL DEFAULT 0,
            audience TEXT NOT NULL DEFAULT 'public',
            hashtags TEXT NOT NULL DEFAULT '[]',
            created_at INTEGER NOT NULL DEFAULT 0,
            position INTEGER NOT NULL DEFAULT 0,
            added_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (playlist_id, track_id)
        );
        CREATE INDEX IF NOT EXISTS idx_group_rooms_group ON group_rooms(group_id, position ASC);
        CREATE INDEX IF NOT EXISTS idx_group_threads_group ON group_threads(group_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_threads_pinned ON group_threads(group_id, is_pinned DESC, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_thread_replies_thread ON group_thread_replies(thread_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_group_thread_replies_parent ON group_thread_replies(parent_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_group_voice_channels_group ON group_voice_channels(group_id, position ASC);
        CREATE INDEX IF NOT EXISTS idx_group_voice_presence_pubkey ON group_voice_presence(pubkey);
        CREATE INDEX IF NOT EXISTS idx_gtr_thread ON group_thread_reactions(thread_id);
        CREATE INDEX IF NOT EXISTS idx_gtr_reply ON group_thread_reactions(reply_id);
        CREATE INDEX IF NOT EXISTS idx_grr_room ON group_room_reactions(group_id, room_id);
        CREATE INDEX IF NOT EXISTS idx_grr_msg ON group_room_reactions(message_id);
        CREATE INDEX IF NOT EXISTS idx_saved_content_saved_at ON saved_content (saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_saved_content_kind_saved ON saved_content (kind, saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_playlist_tracks_pos ON musicloud_playlist_tracks (playlist_id, position);",
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

    // Ensure posts_fts includes the category column if it was created in older builds
    let posts_fts_exists: bool = crate::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='posts_fts'",
        (),
        |r| r.get::<i64>(0),
    )?
    .unwrap_or(0)
        > 0;
    if posts_fts_exists {
        let has_category: bool = crate::query::query_first(
            conn,
            "SELECT COUNT(*) FROM pragma_table_info('posts_fts') WHERE name='category'",
            (),
            |r| r.get::<i64>(0),
        )?
        .unwrap_or(0)
            > 0;
        if !has_category {
            block_on(conn.execute_batch(
                "DROP TRIGGER IF EXISTS posts_ai;
                DROP TRIGGER IF EXISTS posts_au;
                DROP TABLE IF EXISTS posts_fts;

                CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
                    id UNINDEXED,
                    pubkey UNINDEXED,
                    content,
                    subject,
                    category,
                    tokenize='unicode61 remove_diacritics 2'
                );

                CREATE TRIGGER IF NOT EXISTS posts_ai AFTER INSERT ON posts WHEN new.is_deleted = 0 BEGIN
                    INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject, category)
                    VALUES (new.rowid, new.id, new.pubkey, new.content, new.subject, new.category);
                END;

                CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE OF content, subject, category, is_deleted ON posts
                WHEN old.content != new.content OR old.subject IS NOT new.subject OR old.category IS NOT new.category OR old.is_deleted != new.is_deleted
                BEGIN
                    DELETE FROM posts_fts WHERE rowid = old.rowid;
                    INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject, category)
                    SELECT new.rowid, new.id, new.pubkey, new.content, new.subject, new.category WHERE new.is_deleted = 0;
                END;

                INSERT INTO posts_fts(rowid, id, pubkey, content, subject, category)
                SELECT rowid, id, pubkey, content, subject, category FROM posts WHERE is_deleted = 0;

                INSERT INTO posts_fts(posts_fts) VALUES('rebuild');",
            ))?;
        }
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
    let steps: &[(i64, StepFn)] = &[(1, |c| v1_create_tables(c).map_err(Into::into))];

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
