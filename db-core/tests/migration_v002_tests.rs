//! Migration 2 tests: key-column normalization backfill
//! (optimization plan 2026-09-29, item 2.2).
//!
//! These exercise the migration directly against a v1-shaped database holding
//! deliberately mixed-case data, rather than going through `migrate()`, so the
//! pre-state is known exactly.

use soshal_db_core::schema::migrations::{v1_create_tables, v2_normalize_keys};

/// v1-shaped in-memory database, empty.
///
/// A single `Connection` is built and held for the whole test. `Database`
/// pools connections and an extra `connect()` on `:memory:` gets a *fresh
/// empty* database, so this deliberately bypasses the pool — these tests
/// operate on one connection throughout.
fn v1_db() -> libsql::Connection {
    let db = soshal_db_core::block_on(libsql::Builder::new_local(":memory:").build()).unwrap();
    let conn = db.connect().unwrap();
    soshal_db_core::block_on(conn.execute_batch("PRAGMA foreign_keys=ON")).unwrap();
    // `migrate()` creates `_migrations` before running any step, and both
    // migration functions record their version into it. Mirror that here.
    soshal_db_core::block_on(conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')))",
    ))
    .unwrap();
    v1_create_tables(&conn).unwrap();
    conn
}

/// Run a migration step the way `migrate()` does: inside `BEGIN IMMEDIATE`,
/// with the step rolled back on failure. `v2_normalize_keys` opens its own
/// `SAVEPOINT` so it is also safe to call bare; see the standalone test.
fn run_step(conn: &libsql::Connection, step: fn(&libsql::Connection) -> Result<(), libsql::Error>) {
    exec(conn, "BEGIN IMMEDIATE");
    if let Err(e) = step(conn) {
        let _ = soshal_db_core::block_on(conn.execute_batch("ROLLBACK"));
        panic!("migration step failed: {e}");
    }
    exec(conn, "COMMIT");
}

fn exec(conn: &libsql::Connection, sql: &str) {
    soshal_db_core::block_on(conn.execute_batch(sql)).unwrap();
}

fn scalar_i64(conn: &libsql::Connection, sql: &str) -> i64 {
    soshal_db_core::block_on(async {
        let mut rows = conn.query(sql, ()).await.unwrap();
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
    })
}

fn scalar_text(conn: &libsql::Connection, sql: &str) -> String {
    soshal_db_core::block_on(async {
        let mut rows = conn.query(sql, ()).await.unwrap();
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap()
    })
}

const MIXED_PK: &str = "AABBCCDD0011";
const MIXED_ID: &str = "DEADBEEF0001";

fn seed_user_and_post(conn: &libsql::Connection) {
    exec(
        conn,
        &format!("INSERT INTO users (pubkey, npub) VALUES ('{MIXED_PK}', 'npub1x')"),
    );
    exec(
        conn,
        &format!(
            "INSERT INTO posts (id, pubkey, content, kind, created_at)
             VALUES ('{MIXED_ID}', '{MIXED_PK}', 'hello', 1, 1000)"
        ),
    );
}

#[test]
fn normalizes_existing_mixed_case_rows() {
    let conn = v1_db();
    seed_user_and_post(&conn);
    assert_eq!(scalar_text(&conn, "SELECT pubkey FROM users"), MIXED_PK);

    run_step(&conn, v2_normalize_keys);

    assert_eq!(
        scalar_text(&conn, "SELECT pubkey FROM users"),
        "aabbccdd0011"
    );
    assert_eq!(scalar_text(&conn, "SELECT id FROM posts"), "deadbeef0001");
    assert_eq!(
        scalar_text(&conn, "SELECT pubkey FROM posts"),
        "aabbccdd0011"
    );
}

/// The load-bearing test. `PRAGMA foreign_keys` is a no-op inside a
/// transaction and every migration step runs inside `migrate()`'s
/// `BEGIN IMMEDIATE`, so the migration relies on `defer_foreign_keys` instead.
/// If that pragma is a no-op in libsql, normalizing `posts.pubkey` before
/// `users.pubkey` raises an FK violation and this test fails.
#[test]
fn foreign_keys_hold_when_parent_and_child_both_normalize() {
    let conn = v1_db();
    seed_user_and_post(&conn);

    // Same ordering constraint the migration runs under.
    exec(&conn, "BEGIN IMMEDIATE");
    soshal_db_core::block_on(conn.execute_batch("PRAGMA defer_foreign_keys=ON")).unwrap();
    soshal_db_core::block_on(conn.execute(
        "UPDATE posts SET pubkey = LOWER(TRIM(pubkey)) WHERE pubkey <> LOWER(TRIM(pubkey))",
        (),
    ))
    .unwrap();
    soshal_db_core::block_on(conn.execute(
        "UPDATE users SET pubkey = LOWER(TRIM(pubkey)) WHERE pubkey <> LOWER(TRIM(pubkey))",
        (),
    ))
    .unwrap();
    soshal_db_core::block_on(conn.execute_batch("COMMIT")).unwrap();

    let violations: i64 = soshal_db_core::block_on(async {
        let mut rows = conn.query("PRAGMA foreign_key_check", ()).await.unwrap();
        match rows.next().await.unwrap() {
            None => 0,
            Some(_) => 1,
        }
    });
    assert_eq!(violations, 0, "foreign_key_check must report no violations");
}

/// Composite-PK dedupe must group on the *whole* key. `ignored_notifications`
/// is keyed `(pubkey, kind, from_pubkey, event_id)`, so two rows differing
/// only in `from_pubkey` are legitimately distinct. Grouping on `pubkey` alone
/// would silently delete one of them — data loss, not a normalization bug.
#[test]
fn composite_pk_dedupe_keeps_distinct_rows() {
    let conn = v1_db();
    exec(
        &conn,
        "INSERT INTO ignored_notifications (pubkey, kind, from_pubkey, event_id, created_at) VALUES
           ('AABBCC', 'user', 'DDD111', 'EEE222', 1),
           ('AABBCC', 'user', 'DDD999', 'EEE222', 2);",
    );
    assert_eq!(
        scalar_i64(&conn, "SELECT COUNT(*) FROM ignored_notifications"),
        2
    );

    run_step(&conn, v2_normalize_keys);

    assert_eq!(
        scalar_i64(&conn, "SELECT COUNT(*) FROM ignored_notifications"),
        2,
        "rows differing in from_pubkey are distinct under the composite PK and \
         must both survive"
    );
    assert_eq!(
        scalar_i64(
            &conn,
            "SELECT COUNT(*) FROM ignored_notifications WHERE pubkey <> LOWER(pubkey)"
        ),
        0
    );
}

/// True collisions — two rows differing only by case — must collapse to one,
/// otherwise the `UPDATE` aborts on the primary key.
#[test]
fn case_only_collision_collapses_to_one_row() {
    let conn = v1_db();
    exec(
        &conn,
        "INSERT INTO users (pubkey, npub) VALUES ('AABBCC', 'npub1');
         INSERT INTO users (pubkey, npub) VALUES ('aabbcc', 'npub2');",
    );
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM users"), 2);

    run_step(&conn, v2_normalize_keys);

    assert_eq!(
        scalar_i64(&conn, "SELECT COUNT(*) FROM users"),
        1,
        "case-only duplicate must collapse, not abort the UPDATE"
    );
    assert_eq!(scalar_text(&conn, "SELECT npub FROM users"), "npub2");
}

#[test]
fn is_idempotent() {
    let conn = v1_db();
    seed_user_and_post(&conn);
    run_step(&conn, v2_normalize_keys);
    let after_first = scalar_i64(&conn, "SELECT COUNT(*) FROM posts");
    run_step(&conn, v2_normalize_keys);
    assert_eq!(scalar_i64(&conn, "SELECT COUNT(*) FROM posts"), after_first);
}

#[test]
fn records_its_own_version() {
    let conn = v1_db();
    run_step(&conn, v2_normalize_keys);
    assert_eq!(scalar_i64(&conn, "SELECT MAX(version) FROM _migrations"), 2);
}

/// The step must be correct in both call shapes. `migrate()` wraps steps in
/// `BEGIN IMMEDIATE`, but a bare call has to work too — `SAVEPOINT` supplies
/// the transaction, and without that the parent-then-child update order raises
/// `FOREIGN KEY constraint failed` from whichever table is visited first.
#[test]
fn runs_correctly_outside_an_explicit_transaction() {
    let conn = v1_db();
    seed_user_and_post(&conn);
    v2_normalize_keys(&conn).unwrap();
    assert_eq!(
        scalar_text(&conn, "SELECT pubkey FROM users"),
        "aabbccdd0011"
    );
    assert_eq!(
        scalar_text(&conn, "SELECT pubkey FROM posts"),
        "aabbccdd0011"
    );
}
