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
fn v2_creates_group_messages() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v2_create_group_messages(&conn).unwrap();
    assert!(table_exists(&conn, "group_messages"));
    assert_eq!(max_version(&conn), 2);
}

#[test]
fn v3_creates_social_tables() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v3_create_social_tables(&conn).unwrap();
    for t in ["polls", "poll_votes", "marketplace_reviews", "spam_reports"] {
        assert!(table_exists(&conn, t), "missing table {t}");
    }
    assert_eq!(max_version(&conn), 3);
}

#[test]
fn v4_creates_sync_tables() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v4_create_sync_tables(&conn).unwrap();
    for t in ["tx_nodes", "tx_edges", "outbox_queue"] {
        assert!(table_exists(&conn, t), "missing table {t}");
    }
    assert_eq!(max_version(&conn), 4);
}

#[test]
fn v5_creates_missing_tables() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v5_create_missing_tables(&conn).unwrap();
    for t in [
        "diagnostic_logs",
        "do_not_refetch_items",
        "custom_profile_nodes",
        "geohash_peers",
        "friend_backups",
        "link_previews",
        "stream_chat",
        "guestbook_entries",
        "huddle_posts",
        "banned_members",
        "group_join_requests",
        "group_invites",
        "musiclouds",
        "musicloud_comments",
        "story_reactions",
        "muted_conversations",
        "dating_unmatches",
    ] {
        assert!(table_exists(&conn, t), "missing table {t}");
    }
    assert_eq!(max_version(&conn), 5);
}

#[test]
fn v7_rebuilds_fts_triggers() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v7_rebuild_fts_triggers(&conn).unwrap();
    for t in ["posts_ai", "posts_ad", "posts_au"] {
        assert!(trigger_exists(&conn, t), "missing trigger {t}");
    }
    assert_eq!(max_version(&conn), 7);
}

#[test]
fn v8_adds_shared_keys_and_escrow_confirms() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v8_shared_keys_escrow_confirms(&conn).unwrap();
    assert!(table_exists(&conn, "group_shared_keys"));
    assert!(column_exists(&conn, "escrows", "buyer_confirmed"));
    assert!(column_exists(&conn, "escrows", "seller_confirmed"));
    assert_eq!(max_version(&conn), 8);
}

#[test]
fn v9_adds_users_fts() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v9_users_fts(&conn).unwrap();
    assert!(table_exists(&conn, "users_fts"));
    for t in ["users_ai", "users_ad", "users_au"] {
        assert!(trigger_exists(&conn, t), "missing trigger {t}");
    }
    assert_eq!(max_version(&conn), 9);
}

#[test]
fn v10_adds_perf_indexes() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v2_create_group_messages(&conn).unwrap();
    migrations::v4_create_sync_tables(&conn).unwrap();
    migrations::v5_create_missing_tables(&conn).unwrap();
    migrations::v10_perf_indexes(&conn).unwrap();
    for i in [
        "idx_group_messages_fetch",
        "idx_outbox_queue_pending",
        "idx_outbox_queue_created",
        "idx_messages_conv_deleted",
        "idx_reminders_start",
        "idx_geohash_peers_last_seen",
        "idx_huddle_posts_expires",
        "idx_diagnostic_logs_created",
    ] {
        assert!(index_exists(&conn, i), "missing index {i}");
    }
    assert_eq!(max_version(&conn), 10);
}

#[test]
fn v11_adds_conversations_and_post_columns() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v5_create_missing_tables(&conn).unwrap();
    migrations::v11_perf_schema(&conn).unwrap();
    assert!(table_exists(&conn, "conversations"));
    assert!(column_exists(&conn, "posts", "rsvp_event_id"));
    assert!(column_exists(&conn, "reminders", "trigger_at"));
    assert!(index_exists(&conn, "idx_notifications_unread_type"));
    assert_eq!(max_version(&conn), 11);
}

#[test]
fn v12_adds_post_category_and_follower_count() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v5_create_missing_tables(&conn).unwrap();
    migrations::v11_perf_schema(&conn).unwrap();
    migrations::v12_perf_schema(&conn).unwrap();
    assert!(column_exists(&conn, "posts", "category"));
    assert!(column_exists(&conn, "users", "follower_count"));
    assert_eq!(max_version(&conn), 12);
}

#[test]
fn full_chain_reaches_schema_version() {
    let (_db, conn) = bare_db();
    migrations::v1_create_tables(&conn).unwrap();
    migrations::v2_create_group_messages(&conn).unwrap();
    migrations::v3_create_social_tables(&conn).unwrap();
    migrations::v4_create_sync_tables(&conn).unwrap();
    migrations::v5_create_missing_tables(&conn).unwrap();
    migrations::v6_purge_orphan_fts_rows(&conn).unwrap();
    migrations::v7_rebuild_fts_triggers(&conn).unwrap();
    migrations::v8_shared_keys_escrow_confirms(&conn).unwrap();
    migrations::v9_users_fts(&conn).unwrap();
    migrations::v10_perf_indexes(&conn).unwrap();
    migrations::v11_perf_schema(&conn).unwrap();
    migrations::v12_perf_schema(&conn).unwrap();
    assert_eq!(max_version(&conn), SCHEMA_VERSION);
}
