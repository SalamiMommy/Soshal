//! Misc coverage for db-core public surface not exercised by db_tests.rs:
//! TursoState lifecycle, configure/sync_turso, shrink_memory, temp-file open,
//! with_tx commit/rollback, sync query_capacity, clamp helpers, max_connections.

use soshal_db_core::error::DbError;
use soshal_db_core::libsql::params;
use soshal_db_core::query;
use soshal_db_core::repos::{clamp_limit, clamp_page};
use soshal_db_core::turso::TursoState;
use soshal_db_core::{max_connections, Database};

fn migrated_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

fn insert_user(db: &Database, pubkey: &str) {
    let conn = db.conn().unwrap();
    query::execute(
        &conn,
        "INSERT INTO users (pubkey, npub, created_at, updated_at) VALUES (?1,?2,?3,?4)",
        params![pubkey, format!("npub_{pubkey}"), 1000i64, 1000i64],
    )
    .unwrap();
}

#[test]
fn turso_state_lifecycle() {
    let state = TursoState::new();
    let idle = state.status();
    assert!(!idle.configured);
    assert_eq!(idle.status, "idle");
    assert!(idle.last_synced_at.is_none());
    assert!(idle.last_error.is_none());

    state.set_configured(true);
    assert!(state.status().configured);

    state.set_syncing();
    let syncing = state.status();
    assert_eq!(syncing.status, "syncing");
    assert!(syncing.last_error.is_none());

    state.set_synced(12345);
    let synced = state.status();
    assert_eq!(synced.status, "synced");
    assert_eq!(synced.last_synced_at, Some(12345));
    assert!(synced.last_error.is_none());
    assert!(
        synced.configured,
        "configured flag survives sync transitions"
    );
}

#[test]
fn turso_state_error_transition() {
    let state = TursoState::new();
    state.set_configured(true);
    state.set_syncing();
    state.set_error("boom");
    let err = state.status();
    assert_eq!(err.status, "error");
    assert_eq!(err.last_error.as_deref(), Some("boom"));
    assert!(err.configured, "error does not clear configured flag");

    // Recovery: syncing clears the error.
    state.set_syncing();
    let recovered = state.status();
    assert_eq!(recovered.status, "syncing");
    assert!(recovered.last_error.is_none());
}

#[test]
fn configure_turso_marks_configured() {
    let db = migrated_db();
    assert!(!db.turso_status().configured);
    db.configure_turso("libsql://example.turso.io", "tok_123")
        .unwrap();
    let status = db.turso_status();
    assert!(status.configured);
    assert_eq!(
        status.status, "idle",
        "configure alone must not flip status"
    );
}

#[test]
fn sync_turso_without_config_errors() {
    let db = migrated_db();
    let err = db.sync_turso().unwrap_err();
    match err {
        DbError::TursoSync(msg) => assert_eq!(msg, "Turso credentials not configured"),
        other => panic!("expected TursoSync error, got {other:?}"),
    }
    let status = db.turso_status();
    assert_eq!(status.status, "error");
    assert_eq!(
        status.last_error.as_deref(),
        Some("Turso credentials not configured")
    );
    assert!(!status.configured);
}

#[test]
fn sync_turso_with_config_succeeds() {
    let db = migrated_db();
    db.configure_turso("libsql://example.turso.io", "tok_123")
        .unwrap();
    let out = db.sync_turso().unwrap();
    assert!(
        out.contains("Turso sync complete"),
        "unexpected output: {out}"
    );
    assert!(out.contains("libsql://example.turso.io"));
    let status = db.turso_status();
    assert_eq!(status.status, "synced");
    assert!(status.last_synced_at.is_some());
    assert!(status.last_error.is_none());
    assert!(status.configured);
}

#[test]
fn shrink_memory_ok() {
    let db = migrated_db();
    db.shrink_memory().unwrap();
    // Idempotent: a second shrink on a live pool is also fine.
    db.shrink_memory().unwrap();
}

#[test]
fn open_temp_file_migrates_and_works() {
    let unique = format!(
        "soshal_db_test_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    let path_str = path.to_str().unwrap().to_string();

    let db = Database::open(&path_str).unwrap();
    db.migrate().unwrap();
    insert_user(&db, "file_pk");
    let conn = db.conn().unwrap();
    let n: i64 = query::query_first(&conn, "SELECT COUNT(*) FROM users", (), |r| r.get(0))
        .unwrap()
        .unwrap();
    assert_eq!(n, 1);
    drop(conn);
    drop(db);

    // WAL/shm siblings may exist alongside; clean up best-effort.
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path_str}-wal"));
    let _ = std::fs::remove_file(format!("{path_str}-shm"));
}

#[test]
fn with_tx_commit_persists() {
    let db = migrated_db();
    let conn = db.conn().unwrap();
    query::with_tx(&conn, |tx| async move {
        tx.execute(
            "INSERT INTO users (pubkey, npub, created_at, updated_at) VALUES (?1,?2,?3,?4)",
            params!["tx_commit", "npub_tx_commit", 1000i64, 1000i64],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    })
    .unwrap();

    let n: i64 = query::query_first(
        &conn,
        "SELECT COUNT(*) FROM users WHERE pubkey='tx_commit'",
        (),
        |r| r.get(0),
    )
    .unwrap()
    .unwrap();
    assert_eq!(n, 1, "committed tx insert must be visible");
}

#[test]
fn with_tx_rollback_discards() {
    let db = migrated_db();
    let conn = db.conn().unwrap();
    let err = query::with_tx::<(), _>(&conn, |tx| async move {
        tx.execute(
            "INSERT INTO users (pubkey, npub, created_at, updated_at) VALUES (?1,?2,?3,?4)",
            params!["tx_rollback", "npub_tx_rollback", 1000i64, 1000i64],
        )
        .await?;
        Err(DbError::NotFound)
    })
    .unwrap_err();
    assert!(matches!(err, DbError::NotFound));

    let n: i64 = query::query_first(
        &conn,
        "SELECT COUNT(*) FROM users WHERE pubkey='tx_rollback'",
        (),
        |r| r.get(0),
    )
    .unwrap()
    .unwrap();
    assert_eq!(n, 0, "rolled-back tx insert must not be visible");
}

#[test]
fn query_capacity_sync_fetch() {
    let db = migrated_db();
    insert_user(&db, "cap_1");
    insert_user(&db, "cap_2");
    insert_user(&db, "cap_3");

    let conn = db.conn().unwrap();
    // Capacity smaller than the result set: preallocation hint only, never truncation.
    let small = query::query_capacity(
        &conn,
        "SELECT pubkey FROM users ORDER BY pubkey",
        (),
        1,
        |r| r.get::<String>(0),
    )
    .unwrap();
    assert_eq!(small.len(), 3);
    // Larger capacity: still all rows.
    let big = query::query_capacity(
        &conn,
        "SELECT pubkey FROM users ORDER BY pubkey",
        (),
        64,
        |r| r.get::<String>(0),
    )
    .unwrap();
    assert_eq!(big, small);
    assert_eq!(big[0], "cap_1");
    assert_eq!(big[2], "cap_3");
}

#[test]
fn clamp_limit_edges() {
    assert_eq!(clamp_limit(1), 1);
    assert_eq!(clamp_limit(2000), 2000);
    assert_eq!(clamp_limit(0), 1);
    assert_eq!(clamp_limit(-5), 1);
    assert_eq!(clamp_limit(2001), 2000);
    assert_eq!(clamp_limit(i64::MIN), 1);
    assert_eq!(clamp_limit(i64::MAX), 2000);
}

#[test]
fn clamp_page_edges() {
    assert_eq!(clamp_page(10, 5), (10, 5));
    assert_eq!(
        clamp_page(0, -3),
        (1, 0),
        "limit clamps up, negative offset clamps to 0"
    );
    assert_eq!(clamp_page(5000, 0), (2000, 0));
    assert_eq!(
        clamp_page(-1, i64::MAX),
        (1, i64::MAX),
        "offset has no upper bound"
    );
}

#[test]
fn max_connections_in_range() {
    let n = max_connections();
    assert!((4..=16).contains(&n), "max_connections out of range: {n}");
    assert_eq!(soshal_db_core::MAX_CONNECTIONS, 4);
}
