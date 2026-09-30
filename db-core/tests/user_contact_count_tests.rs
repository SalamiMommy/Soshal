//! The SQL-derived `UserRow::contact_count` (optimization plan 2026-09-29,
//! item 6.10).
//!
//! `contact_pubkeys` holds one JSON string per contact. Reporting a follow
//! count used to parse it in Rust — `serde_json::from_str::<Vec<String>>` —
//! which allocates a `String` per entry to produce one `i32`. Since
//! `row_to_profile` runs once per requested pubkey, a 200-entry profile grid
//! allocated 200 × N strings. The count is now `json_array_length` in the
//! SELECT projection, so the logic under test lives in SQL, not Rust.
//!
//! These tests therefore assert the *value that came back from the database*
//! for each projection, not the expression. The expression is a private macro;
//! a test that restates it proves nothing.

use soshal_db_core::repos::user::{UserRepo, UserRow};
use soshal_db_core::Database;

/// `db-core` gotcha (see AGENTS.md): `open_in_memory` pools connections, so an
/// extra `connect()` can get a *fresh empty* in-memory database. Write first,
/// read afterwards, never hold a `conn()` across a repo call.
fn seeded() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.migrate().unwrap();
    db
}

fn user(pubkey: &str, contacts: &str) -> UserRow {
    UserRow {
        pubkey: pubkey.into(),
        npub: format!("npub_{pubkey}"),
        name: Some("Alice".into()),
        display_name: None,
        about: None,
        picture: None,
        banner: None,
        nip05: None,
        lud16: None,
        created_at: 1000,
        updated_at: 1000,
        metadata_json: None,
        contact_pubkeys: contacts.into(),
        relay_list: "[]".into(),
        follower_count: 0,
        // Not a column: write paths leave it 0 and the read projection
        // overwrites it from SQL.
        contact_count: 0,
    }
}

fn contacts(n: usize) -> String {
    let v: Vec<String> = (0..n).map(|i| format!("\"{i:064x}\"")).collect();
    format!("[{}]", v.join(","))
}

#[test]
fn batch_read_counts_contacts_in_sql() {
    let db = seeded();
    let repo = UserRepo::new(&db);
    repo.upsert(&user(&"a".repeat(64), &contacts(3))).unwrap();
    repo.upsert(&user(&"b".repeat(64), &contacts(0))).unwrap();
    repo.upsert(&user(&"c".repeat(64), &contacts(1))).unwrap();

    let rows = repo
        .rows_for_pubkeys(&["a".repeat(64), "b".repeat(64), "c".repeat(64)])
        .unwrap();

    assert_eq!(rows[&"a".repeat(64)].contact_count, 3);
    assert_eq!(rows[&"b".repeat(64)].contact_count, 0);
    assert_eq!(rows[&"c".repeat(64)].contact_count, 1);
}

#[test]
fn single_read_counts_contacts_in_sql() {
    let db = seeded();
    let repo = UserRepo::new(&db);
    repo.upsert(&user(&"a".repeat(64), &contacts(7))).unwrap();

    let row = repo.get_by_pubkey(&"a".repeat(64)).unwrap().unwrap();
    assert_eq!(row.contact_count, 7);
}

#[test]
fn fts_search_counts_contacts_in_sql() {
    // The third projection. It joins `users_fts` to `users`, so its column list
    // carries a `u.` prefix — the reason the projection is a macro parameter
    // rather than a second hand-maintained list. A missing count here would
    // shift every column after it, not just the count.
    let db = seeded();
    let repo = UserRepo::new(&db);
    repo.upsert(&user(&"a".repeat(64), &contacts(4))).unwrap();

    let found = repo.search("Alice", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].contact_count, 4);
    // The rest of the projection still decodes correctly, which is the actual
    // protection: a dropped or duplicated column shifts fields, not counts.
    assert_eq!(found[0].name.as_deref(), Some("Alice"));
    assert_eq!(found[0].created_at, 1000);
    assert_eq!(found[0].contact_pubkeys, contacts(4));
}

#[test]
fn malformed_contact_json_counts_zero_instead_of_erroring() {
    // The `json_valid` guard is the whole point. `serde_json::from_str` on
    // "not-json" returned `Err`, which `unwrap_or(0)` turned into 0. A bare
    // `json_array_length` raises a malformed-JSON error in libsql, which would
    // fail the *entire* batch rather than degrade one row.
    let db = seeded();
    let repo = UserRepo::new(&db);
    repo.upsert(&user(&"a".repeat(64), "not-json")).unwrap();
    // A good row in the same batch: one corrupt row must not poison the rest.
    repo.upsert(&user(&"b".repeat(64), &contacts(2))).unwrap();

    let rows = repo
        .rows_for_pubkeys(&["a".repeat(64), "b".repeat(64)])
        .unwrap();
    assert_eq!(rows[&"a".repeat(64)].contact_count, 0);
    assert_eq!(rows[&"b".repeat(64)].contact_count, 2);
}

#[test]
fn valid_non_array_json_counts_zero() {
    // Well-formed JSON that is not an array. `Vec<String>` deserialization
    // failed, so the old path reported 0; `json_array_length` also reports 0
    // for a non-array. Pinned so a future JSON1 change cannot silently start
    // reporting an object key count as a follow count.
    let db = seeded();
    let repo = UserRepo::new(&db);
    repo.upsert(&user(&"a".repeat(64), r#"{"alice": 1, "bob": 2}"#))
        .unwrap();

    let row = repo.get_by_pubkey(&"a".repeat(64)).unwrap().unwrap();
    assert_eq!(row.contact_count, 0);
}

#[test]
fn a_large_contact_list_counts_without_materializing_it() {
    // The case the optimization exists for. 5 000 contacts: the old path
    // allocated 5 000 `String`s per profile row, so a 200-profile grid
    // allocated a million. The count itself is unchanged.
    let db = seeded();
    let repo = UserRepo::new(&db);
    repo.upsert(&user(&"a".repeat(64), &contacts(5000)))
        .unwrap();

    let row = repo.get_by_pubkey(&"a".repeat(64)).unwrap().unwrap();
    assert_eq!(row.contact_count, 5000);
}
