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

// ---------------------------------------------------------------------------
// Why the LOWER() has to go (plan item 2.3)
//
// These are the payoff tests. Removing `LOWER()` from a predicate is only
// worth anything if the query then stops scanning, so assert on the query plan
// rather than on the SQL text — the text is the thing being changed, the plan
// is the thing being bought.
// ---------------------------------------------------------------------------

/// `EXPLAIN QUERY PLAN` detail lines for `sql`, joined.
fn query_plan(db: &Database, sql: &str) -> String {
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(async {
        let mut rows = conn
            .query(&format!("EXPLAIN QUERY PLAN {sql}"), ())
            .await
            .unwrap();
        let mut out = String::new();
        while let Some(row) = rows.next().await.unwrap() {
            out.push_str(&row.get::<String>(3).unwrap());
            out.push('\n');
        }
        out
    })
}

/// SQLite reports a btree lookup as `SEARCH`, and a full pass as `SCAN`.
fn seeks_index(plan: &str) -> bool {
    plan.contains("SEARCH")
}

#[test]
fn post_lookup_by_id_seeks_the_primary_key() {
    let db = seeded();
    let plan = query_plan(&db, "SELECT id FROM posts WHERE id = 'x'");
    assert!(
        seeks_index(&plan),
        "post lookup by primary key should seek, not scan.\nplan:\n{plan}"
    );
}

/// Negative control. `LOWER()` on the column is what forced the scan, so
/// without this the test above would pass even if the predicate regressed
/// back to a scan for an unrelated reason.
#[test]
fn wrapping_the_column_in_lower_forces_a_scan() {
    let db = seeded();
    let plan = query_plan(&db, "SELECT id FROM posts WHERE LOWER(id) = 'x'");
    assert!(
        !seeks_index(&plan),
        "LOWER(id) should not be able to use the index — if this ever starts \
         seeking, the premise of removing LOWER() has changed.\nplan:\n{plan}"
    );
}

#[test]
fn post_lookup_by_author_seeks_an_index() {
    let db = seeded();
    let plan = query_plan(&db, "SELECT id FROM posts WHERE pubkey = 'x'");
    assert!(
        seeks_index(&plan),
        "post lookup by author should seek.\nplan:\n{plan}"
    );
}

#[test]
fn user_lookup_by_pubkey_seeks_the_primary_key() {
    let db = seeded();
    let plan = query_plan(&db, "SELECT npub FROM users WHERE pubkey = 'x'");
    assert!(
        seeks_index(&plan),
        "user lookup by pubkey should seek.\nplan:\n{plan}"
    );
}

#[test]
fn post_upsert_stores_keys_lowercase() {
    use soshal_db_core::repos::post::{PostRepo, PostRow};
    let db = seeded();
    let repo = PostRepo::new(&db);
    let row = PostRow {
        id: MIXED_ID.to_string(),
        pubkey: MIXED_PK.to_string(),
        content: "hello".to_string(),
        kind: 1,
        created_at: 1_000,
        tags_json: "[]".to_string(),
        sig: None,
        reply_to: Some("BBBBCCCC1111".to_string()),
        root_id: Some("DDDDeeee2222".to_string()),
        mentioned_pubkeys: "[]".to_string(),
        mentioned_hashtags: "[]".to_string(),
        subject: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
        rsvp_event_id: None,
    };
    repo.upsert(&row).unwrap();

    assert_column_lowercase(&db, "posts", "id");
    assert_column_lowercase(&db, "posts", "pubkey");
    assert_column_lowercase(&db, "posts", "root_id");
    assert_column_lowercase(&db, "posts", "reply_to");
}

/// A post written with mixed-case keys must still be findable through the
/// index-backed lookups, and through the `json_each` batch path.
#[test]
fn mixed_case_post_is_reachable_after_write() {
    use soshal_db_core::repos::post::{PostRepo, PostRow};
    let db = seeded();
    let repo = PostRepo::new(&db);
    let row = PostRow {
        id: MIXED_ID.to_string(),
        pubkey: MIXED_PK.to_string(),
        content: "hello".to_string(),
        kind: 1,
        created_at: 1_000,
        tags_json: "[]".to_string(),
        sig: None,
        reply_to: None,
        root_id: None,
        mentioned_pubkeys: "[]".to_string(),
        mentioned_hashtags: "[]".to_string(),
        subject: None,
        sync_status: "synced".to_string(),
        is_deleted: false,
        scheduled_at: None,
        freenet_key: None,
        is_freenet_native: false,
        rsvp_event_id: None,
    };
    repo.upsert(&row).unwrap();

    // Looked up with the *original* mixed-case value, which is what a caller
    // holding a relay-supplied id would pass.
    assert!(
        repo.get_by_id(MIXED_ID).unwrap().is_some(),
        "get_by_id must normalize its argument and find the row"
    );
    let by_author = repo.get_user_posts(MIXED_PK, 10, 0).unwrap();
    assert_eq!(by_author.len(), 1, "get_user_posts must find the post");
}
