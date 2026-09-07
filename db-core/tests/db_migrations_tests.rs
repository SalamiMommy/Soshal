use soshal_db_core::libsql::params;
use soshal_db_core::schema::migrations;
use soshal_db_core::schema::SCHEMA_VERSION;
use soshal_db_core::Database;

fn bare_db() -> (Database, soshal_db_core::ConnGuard) {
    let db = Database::open_in_memory().unwrap();
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')));",
    ))
    .unwrap();
    (db, conn)
}

fn table_exists(conn: &soshal_db_core::libsql::Connection, name: &str) -> bool {
    soshal_db_core::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |r| r.get::<i64>(0),
    )
    .unwrap()
    .unwrap_or(0)
        > 0
}

fn column_exists(conn: &soshal_db_core::libsql::Connection, table: &str, column: &str) -> bool {
    soshal_db_core::query::query_first(
        conn,
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
        params![table, column],
        |r| r.get::<i64>(0),
    )
    .unwrap()
    .unwrap_or(0)
        > 0
}

fn index_exists(conn: &soshal_db_core::libsql::Connection, name: &str) -> bool {
    soshal_db_core::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
        params![name],
        |r| r.get::<i64>(0),
    )
    .unwrap()
    .unwrap_or(0)
        > 0
}

fn trigger_exists(conn: &soshal_db_core::libsql::Connection, name: &str) -> bool {
    soshal_db_core::query::query_first(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name=?1",
        params![name],
        |r| r.get::<i64>(0),
    )
    .unwrap()
    .unwrap_or(0)
        > 0
}

fn max_version(conn: &soshal_db_core::libsql::Connection) -> i64 {
    soshal_db_core::query::query_first(
        conn,
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        (),
        |r| r.get(0),
    )
    .unwrap()
    .unwrap_or(0)
}

#[test]
fn v1_creates_base_tables() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    for t in [
        "users",
        "posts",
        "messages",
        "reactions",
        "zaps",
        "notifications",
        "bookmarks",
        "blocks",
        "groups",
        "group_members",
        "settings",
        "relays",
        "posts_fts",
    ] {
        assert!(table_exists(&conn, t), "missing table {t}");
    }
    assert_eq!(max_version(&conn), 1);
}

#[test]
fn full_chain_reaches_schema_version() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v2_group_channels(&conn).unwrap();
    migrations::v3_group_thread_reactions(&conn).unwrap();
    migrations::v4_group_password(&conn).unwrap();
    migrations::v5_performance_indexes(&conn).unwrap();
    migrations::v6_index_cleanup(&conn).unwrap();
    migrations::v7_query_optimizations(&conn).unwrap();
    migrations::v8_index_cleanup(&conn).unwrap();
    migrations::v9_trigger_optimization(&conn).unwrap();
    migrations::create_dating_unmatch_actor_column(&conn).unwrap();
    migrations::v11_index_cleanup(&conn).unwrap();
    migrations::v12_saved_content_playlists(&conn).unwrap();
    migrations::v13_category_fts(&conn).unwrap();
    assert_eq!(max_version(&conn), SCHEMA_VERSION);
    assert!(table_exists(&conn, "group_rooms"));
    assert!(table_exists(&conn, "group_threads"));
    assert!(table_exists(&conn, "group_thread_replies"));
    assert!(table_exists(&conn, "group_thread_reactions"));
    assert!(table_exists(&conn, "group_voice_channels"));
    assert!(table_exists(&conn, "group_voice_presence"));
    assert!(table_exists(&conn, "zk_state_rollups"));
    assert!(!index_exists(&conn, "idx_posts_user_timeline"));
    assert!(!index_exists(&conn, "idx_posts_kind"));
    assert!(!index_exists(&conn, "idx_posts_kind_content_rsvp"));
    assert!(index_exists(&conn, "idx_posts_kind_rsvp"));
    assert!(index_exists(&conn, "idx_messages_conversation_asc"));
    assert!(index_exists(&conn, "idx_reactions_event_pubkey"));
    assert!(index_exists(&conn, "idx_reposts_event_pubkey"));
    assert!(column_exists(&conn, "group_messages", "room_id"));
    assert!(column_exists(&conn, "groups", "password_hash"));
    assert!(column_exists(&conn, "zk_state_rollups", "genesis_root"));
    assert!(index_exists(&conn, "idx_escrows_created"));
    assert!(!index_exists(&conn, "idx_posts_feed_lookup"));
    assert!(!index_exists(&conn, "idx_posts_recent_lookup"));
    assert!(!index_exists(&conn, "idx_posts_kind_created"));
    assert!(!index_exists(&conn, "idx_notifications_unread_type"));
    assert!(!index_exists(&conn, "idx_messages_conv_deleted"));
}

/// Legacy pre-squash database: the four tables that gained columns via ALTER
/// TABLE in old migrations v008-v013, without those columns. Everything else
/// is created fresh by v1_create_tables during migrate.
fn legacy_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE users (
                 pubkey TEXT PRIMARY KEY,
                 npub TEXT NOT NULL,
                 name TEXT,
                 display_name TEXT,
                 about TEXT,
                 picture TEXT,
                 banner TEXT,
                 nip05 TEXT,
                 lud16 TEXT,
                 created_at INTEGER NOT NULL DEFAULT 0,
                 updated_at INTEGER NOT NULL DEFAULT 0,
                 metadata_json TEXT,
                 contact_pubkeys TEXT DEFAULT '[]',
                 relay_list TEXT DEFAULT '[]'
             );
             CREATE TABLE posts (
                 id TEXT PRIMARY KEY,
                 pubkey TEXT NOT NULL REFERENCES users(pubkey),
                 content TEXT NOT NULL DEFAULT '',
                 kind INTEGER NOT NULL DEFAULT 1,
                 created_at INTEGER NOT NULL,
                 tags_json TEXT NOT NULL DEFAULT '[]',
                 sig TEXT,
                 reply_to TEXT,
                 root_id TEXT,
                 mentioned_pubkeys TEXT DEFAULT '[]',
                 mentioned_hashtags TEXT DEFAULT '[]',
                 subject TEXT,
                 sync_status TEXT NOT NULL DEFAULT 'pending',
                 is_deleted INTEGER NOT NULL DEFAULT 0,
                 scheduled_at INTEGER,
                 freenet_key TEXT,
                 is_freenet_native INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE reminders (
                 id TEXT PRIMARY KEY,
                 event_id TEXT NOT NULL,
                 title TEXT NOT NULL,
                 start_time INTEGER NOT NULL,
                 minutes_before INTEGER NOT NULL DEFAULT 10,
                 created_at INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE escrows (
                 id TEXT PRIMARY KEY,
                 listing_id TEXT NOT NULL,
                 buyer_pubkey TEXT NOT NULL,
                 seller_pubkey TEXT NOT NULL,
                 amount_msats INTEGER NOT NULL,
                 currency TEXT NOT NULL DEFAULT 'sats',
                 status TEXT NOT NULL DEFAULT 'created',
                 escrow_note TEXT,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );",
        ))
        .unwrap();
    }
    db
}

const HEALED_COLUMNS: &[(&str, &str)] = &[
    ("posts", "rsvp_event_id"),
    ("posts", "category"),
    ("posts", "reposts_count"),
    ("posts", "event_lat"),
    ("posts", "event_lng"),
    ("reminders", "trigger_at"),
    ("users", "follower_count"),
    ("escrows", "buyer_confirmed"),
    ("escrows", "seller_confirmed"),
];

#[test]
fn legacy_db_heals_missing_columns() {
    let db = legacy_db();
    db.migrate().unwrap();
    let conn = db.conn().unwrap();
    for &(table, column) in HEALED_COLUMNS {
        assert!(
            column_exists(&conn, table, column),
            "missing {table}.{column}"
        );
    }
    assert!(index_exists(&conn, "idx_posts_rsvp_event"));
    assert!(index_exists(&conn, "idx_posts_event_lat_lng"));
    assert!(index_exists(&conn, "idx_users_follower_count"));
    assert!(trigger_exists(&conn, "posts_ai"));
    assert_eq!(max_version(&conn), SCHEMA_VERSION);
}

#[test]
fn legacy_db_with_old_migrations_heals_before_short_circuit() {
    let db = legacy_db();
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(
            conn.execute_batch("INSERT OR IGNORE INTO _migrations (version) VALUES (13);"),
        )
        .unwrap();
    }
    db.migrate().unwrap();
    let conn = db.conn().unwrap();
    for &(table, column) in HEALED_COLUMNS {
        assert!(
            column_exists(&conn, table, column),
            "missing {table}.{column}"
        );
    }
    assert_eq!(max_version(&conn), 13);
    assert!(
        !table_exists(&conn, "messages"),
        "v1 must be skipped when already migrated"
    );
    assert!(
        table_exists(&conn, "musicloud_playlists"),
        "musicloud_playlists must be healed"
    );
    assert!(
        table_exists(&conn, "musicloud_timed_comments"),
        "musicloud_timed_comments must be healed"
    );
}

#[test]
fn heal_legacy_schema_is_idempotent() {
    let db = legacy_db();
    db.migrate().unwrap();
    db.migrate().unwrap();
    let conn = db.conn().unwrap();
    for &(table, column) in HEALED_COLUMNS {
        assert!(
            column_exists(&conn, table, column),
            "missing {table}.{column}"
        );
    }
    assert_eq!(max_version(&conn), SCHEMA_VERSION);
}
