//! Key-column normalization invariant (optimization plan 2026-09-29, item 0.2).
//!
//! Every key-shaped column that the query layer matches *case-insensitively*
//! via `LOWER(col) = LOWER(?)` must be written in lowercase. That is what
//! makes the `LOWER()` removable: a function call on an indexed column makes
//! the btree unusable, so `WHERE LOWER(id) = LOWER(?)` scans instead of
//! hitting the primary key.
//!
//! These tests are written in terms of the public repo API and assert on what
//! actually landed in SQLite, not on the normalization expression itself —
//! that is the thing that matters, and it is the thing that regresses when
//! someone adds a new write site and copies an existing one.

use soshal_db_core::repos::bookmark::{BookmarkRepo, BookmarkRow};
use soshal_db_core::repos::reaction::{ReactionRepo, ReactionRow};
use soshal_db_core::repos::repost::{RepostRepo, RepostRow};
use soshal_db_core::repos::zap::{ZapRepo, ZapRow};
use soshal_db_core::Database;

/// `db-core` gotcha (see AGENTS.md): `open_in_memory` pools connections, so an
/// extra `connect()` can get a *fresh empty* in-memory database. Repo methods
/// take their own connection, so the helper below always writes first and
/// reads afterwards — never hold a `conn()` across a repo call.
fn seeded() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

/// Assert no row in `table` has a non-lowercase (or padded) value in `column`.
fn assert_column_lowercase(db: &Database, table: &str, column: &str) {
    let conn = db.conn().unwrap();
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {column} <> LOWER(TRIM({column}))");
    let offenders: i64 = soshal_db_core::block_on(async {
        let mut rows = conn.query(&sql, ()).await.unwrap();
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
    });
    assert_eq!(
        offenders, 0,
        "{table}.{column} has {offenders} row(s) not stored lowercase — \
         the case-insensitive lookups on this column will diverge from the \
         primary key and the row becomes unreachable from its own lookup"
    );
}

/// Mixed-case input is the whole point: a pubkey or event id arriving with
/// uppercase hex must still be stored lowercase.
const MIXED_PK: &str = "A1B2C3D4e5F6";
const MIXED_ID: &str = "DEADBEEF0011";

#[test]
fn reaction_keys_are_stored_lowercase() {
    let db = seeded();
    let repo = ReactionRepo::new(&db);
    repo.upsert(&ReactionRow {
        id: MIXED_ID.to_string(),
        pubkey: MIXED_PK.to_string(),
        event_id: "FEEDFACE0001".to_string(),
        kind: 7,
        content: Some("+".to_string()),
        created_at: 1_000,
    })
    .unwrap();

    assert_column_lowercase(&db, "reactions", "id");
    assert_column_lowercase(&db, "reactions", "pubkey");
    assert_column_lowercase(&db, "reactions", "event_id");
    // The `INSERT OR IGNORE INTO users` sibling write must normalize too, or
    // `users` and `reactions` disagree about the same author's key.
    assert_column_lowercase(&db, "users", "pubkey");
}

#[test]
fn bookmark_keys_are_stored_lowercase() {
    let db = seeded();
    let repo = BookmarkRepo::new(&db);
    repo.upsert(&BookmarkRow {
        id: MIXED_ID.to_string(),
        pubkey: MIXED_PK.to_string(),
        event_id: "FEEDFACE0002".to_string(),
        created_at: 1_000,
    })
    .unwrap();

    assert_column_lowercase(&db, "bookmarks", "id");
    assert_column_lowercase(&db, "bookmarks", "pubkey");
    assert_column_lowercase(&db, "bookmarks", "event_id");
}

#[test]
fn zap_keys_are_stored_lowercase() {
    let db = seeded();
    let repo = ZapRepo::new(&db);
    repo.upsert(&ZapRow {
        id: MIXED_ID.to_string(),
        pubkey: MIXED_PK.to_string(),
        recipient_pubkey: "F00DBARF00D".to_string(),
        event_id: Some("FEEDFACE0003".to_string()),
        amount: 1_000,
        amount_msat: 1_000_000,
        content: None,
        created_at: 1_000,
        zap_type: "public".to_string(),
    })
    .unwrap();

    assert_column_lowercase(&db, "zaps", "id");
    assert_column_lowercase(&db, "zaps", "pubkey");
    assert_column_lowercase(&db, "zaps", "recipient_pubkey");
    assert_column_lowercase(&db, "zaps", "event_id");
}

#[test]
fn repost_keys_are_stored_lowercase() {
    let db = seeded();
    let repo = RepostRepo::new(&db);
    repo.upsert(&RepostRow {
        id: MIXED_ID.to_string(),
        pubkey: MIXED_PK.to_string(),
        event_id: "FEEDFACE0004".to_string(),
        created_at: 1_000,
    })
    .unwrap();

    assert_column_lowercase(&db, "reposts", "id");
    assert_column_lowercase(&db, "reposts", "pubkey");
    assert_column_lowercase(&db, "reposts", "event_id");
}

/// The bug this file exists to prevent: `reactions.id` was bound as
/// `row.id.trim()` while the dedup `DELETE` and the existing-row probe both
/// compared case-insensitively. A mixed-case id therefore deleted the old row
/// and then inserted under a *different* primary key than the next upsert
/// would collide with — so the table accumulated duplicates that no amount of
/// upserting would ever collapse.
#[test]
fn repeated_mixed_case_upsert_does_not_duplicate() {
    let db = seeded();
    let repo = ReactionRepo::new(&db);
    let row = || ReactionRow {
        id: "AaBbCcDdEe01".to_string(),
        pubkey: "1234AbCdEf56".to_string(),
        event_id: "9999AaBbCcDd".to_string(),
        kind: 7,
        content: Some("+".to_string()),
        created_at: 1_000,
    };

    repo.upsert(&row()).unwrap();
    repo.upsert(&row()).unwrap();
    repo.upsert(&row()).unwrap();

    let conn = db.conn().unwrap();
    let count: i64 = soshal_db_core::block_on(async {
        let mut rows = conn
            .query("SELECT COUNT(*) FROM reactions", ())
            .await
            .unwrap();
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
    });
    assert_eq!(
        count, 1,
        "three upserts of the same reaction must leave one row"
    );
}
