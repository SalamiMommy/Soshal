//! `Database::open` memoizes its pool per path.
//!
//! The bridge global, the sync engine and the mesh ingest task each call
//! `Database::open` with the same file. Before this, that meant three
//! independent libsql pools contending for the WAL write lock. These pin the
//! sharing, the isolation between different paths, and the fact that a dropped
//! pool is not resurrected.

use soshal_db_core::change_bus::Table;
use soshal_db_core::Database;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_path(tag: &str) -> String {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("soshal_pool_{tag}_{}_{n}.db", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
}

/// Count rows in a table that may not exist, returning `None` when the table
/// is absent — the signal these tests actually assert on.
fn count_rows(db: &Database, table: &str) -> Option<i64> {
    let conn = db.conn().unwrap();
    let sql = format!("SELECT COUNT(*) FROM {table}");
    soshal_db_core::block_on(async {
        let mut rows = conn.query(&sql, ()).await.ok()?;
        let row = rows.next().await.ok()??;
        row.get::<i64>(0).ok()
    })
}

#[test]
fn same_path_shares_one_pool() {
    let path = temp_path("shared");
    let a = Database::open(&path).unwrap();
    let b = Database::open(&path).unwrap();

    // The change bus is the observable that actually distinguishes a shared
    // pool from two pools on one file: it lives on `PoolInner`, so a notify
    // through one handle reaches subscribers on the other only when they share
    // a pool. (A plain read of a written row would NOT distinguish them — two
    // pools against the same file see the same bytes.)
    //
    // This is also the behaviour we want, not just a sharing detail: the sync
    // engine and the mesh ingest task write through their own handles, and
    // before this change their writes never woke the bridge's reactive
    // subscribers.
    let mut rx = a.subscribe_changes();
    b.notify_change(Table::Posts, None);
    let got = soshal_db_core::block_on(async {
        tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
    });
    assert!(
        got.is_ok(),
        "a change notified through one handle must reach a subscriber on the other"
    );

    // And the pool is genuinely connected, not a shared husk.
    soshal_db_core::block_on(
        a.conn()
            .unwrap()
            .execute("CREATE TABLE shared_marker (x INTEGER)", ()),
    )
    .unwrap();
    assert_eq!(count_rows(&b, "shared_marker"), Some(0));

    drop(a);
    drop(b);
    cleanup(&path);
}

/// The negative control for the test above: two *different* paths must not
/// share a bus. Without this, `same_path_shares_one_pool` could pass for the
/// wrong reason — a single global bus rather than a per-path one.
#[test]
fn different_paths_have_independent_change_buses() {
    let pa = temp_path("bus_a");
    let pb = temp_path("bus_b");
    let a = Database::open(&pa).unwrap();
    let b = Database::open(&pb).unwrap();

    let mut rx = a.subscribe_changes();
    b.notify_change(Table::Posts, None);
    let got = soshal_db_core::block_on(async {
        tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
    });
    assert!(
        got.is_err(),
        "a notify on one path must not reach subscribers of another"
    );

    drop(a);
    drop(b);
    cleanup(&pa);
    cleanup(&pb);
}

#[test]
fn different_paths_do_not_share() {
    let pa = temp_path("iso_a");
    let pb = temp_path("iso_b");
    let a = Database::open(&pa).unwrap();
    let b = Database::open(&pb).unwrap();

    a.migrate().unwrap();
    b.migrate().unwrap();

    // If these shared a pool, the second migrate would be operating on the
    // first's file. Write a table only in `a` and prove `b` cannot see it.
    soshal_db_core::block_on(
        a.conn()
            .unwrap()
            .execute("CREATE TABLE only_in_a (x INTEGER)", ()),
    )
    .unwrap();
    assert_eq!(
        count_rows(&b, "only_in_a"),
        None,
        "a second path must not see the first path's tables"
    );

    drop(a);
    drop(b);
    cleanup(&pa);
    cleanup(&pb);
}

/// The registry holds `Weak`, so a pool nothing references is released rather
/// than pinned for the life of the process.
///
/// Note what is *not* asserted here: the data survives the drop, because the
/// database is a file and reopening it reopens the same file. Pool identity
/// after a full drop is not observable from outside the crate — that is
/// precisely why the registry is `Weak` plus a sweep rather than `Arc`, since
/// a strong reference would be an unbounded leak no test could see. What this
/// pins is the part that is observable: a reopened path yields a working pool
/// and sees everything committed before the drop.
#[test]
fn reopening_after_a_full_drop_yields_a_working_pool() {
    let path = temp_path("resurrect");
    {
        let a = Database::open(&path).unwrap();
        a.migrate().unwrap();
        soshal_db_core::block_on(
            a.conn()
                .unwrap()
                .execute("CREATE TABLE marker (x INTEGER)", ()),
        )
        .unwrap();
    } // every handle dropped here

    let b = Database::open(&path).unwrap();
    assert_eq!(
        count_rows(&b, "marker"),
        Some(0),
        "committed data is still there after the pool is released and rebuilt"
    );
    // And the rebuilt pool is live, not a stale husk: new writes land.
    soshal_db_core::block_on(
        b.conn()
            .unwrap()
            .execute("INSERT INTO marker (x) VALUES (7)", ()),
    )
    .unwrap();
    assert_eq!(count_rows(&b, "marker"), Some(1));
    drop(b);
    cleanup(&path);
}

/// In-memory databases are deliberately excluded from the memoization: two
/// `open_in_memory` calls are two distinct databases by contract, and the
/// pooling gotcha around extra `connect()`s makes conflating them worse than
/// the duplication costs.
#[test]
fn in_memory_databases_stay_distinct() {
    let a = Database::open_in_memory().unwrap();
    let b = Database::open_in_memory().unwrap();
    soshal_db_core::block_on(
        a.conn()
            .unwrap()
            .execute("CREATE TABLE only_in_memory (x INTEGER)", ()),
    )
    .unwrap();
    assert_eq!(
        count_rows(&b, "only_in_memory"),
        None,
        "two open_in_memory handles must not alias each other"
    );
}
