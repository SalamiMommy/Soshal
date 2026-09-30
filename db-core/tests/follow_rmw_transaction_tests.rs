//! The transaction shape that `identity_follow_user` / `identity_unfollow_user`
//! use: read the contact list, modify it, write it, and adjust the target's
//! follower count — all inside one IMMEDIATE transaction.
//!
//! The old code spread that across three transactions (a `with_db_result` that
//! read, a second that wrote the list, and a third inside
//! `bump_follower_count`), so two concurrent callers could both read
//! "not following" and both write a list missing the target — or, worse, both
//! bump the target's follower count.
//!
//! These tests pin the property the fix restores: with the whole
//! read-modify-write inside one IMMEDIATE transaction, concurrent writers
//! serialize and the *later* one observes the *earlier* one's write.

use soshal_db_core::repos::user::UserRepo;
use soshal_db_core::Database;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_db(tag: &str) -> (Database, String) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!("soshal_follow_{tag}_{}_{n}.db", std::process::id()))
        .to_string_lossy()
        .into_owned();
    let db = Database::open(&path).unwrap();
    db.migrate().unwrap();
    (db, path)
}

fn cleanup(path: &str) {
    for p in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        let _ = std::fs::remove_file(&p);
    }
}

fn pk(n: u32) -> String {
    format!("{n:064x}")
}

fn follow_count(db: &Database, signer: &str, target: &str) -> usize {
    let conn = db.conn().unwrap();
    let list: String = soshal_db_core::block_on(async {
        let mut r = conn
            .query(
                "SELECT contact_pubkeys FROM users WHERE pubkey = ?1",
                [signer],
            )
            .await
            .unwrap();
        match r.next().await.unwrap() {
            Some(row) => row.get::<String>(0).unwrap(),
            None => String::new(),
        }
    });
    if list.is_empty() {
        return 0;
    }
    let parsed: Vec<String> = serde_json::from_str(&list).unwrap_or_default();
    parsed
        .iter()
        .filter(|p| p.eq_ignore_ascii_case(target))
        .count()
}

fn seed_signer(db: &Database, signer: &str) {
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(conn.execute(
        "INSERT INTO users (pubkey, npub, contact_pubkeys, relay_list) \
         VALUES (?1, '', '[]', '[]')",
        [signer],
    ))
    .unwrap();
}

/// One follow, expressed the way the fixed FFI path does it: a single IMMEDIATE
/// transaction holding the read, the append, the list write and the count bump.
fn follow_in_one_tx(db: &Database, signer: &str, target: &str) -> bool {
    let conn = db.conn().unwrap();
    let repo = UserRepo::new(db);
    let (signer, target) = (signer.to_string(), target.to_string());
    soshal_db_core::query::with_tx(&conn, |tx| async move {
        let Some(mut row) = repo.get_by_pubkey_in(&tx, &signer).await.unwrap() else {
            return Ok(false);
        };
        let mut follows: Vec<String> =
            serde_json::from_str(&row.contact_pubkeys).unwrap_or_default();
        let was_following = follows.iter().any(|f| f.eq_ignore_ascii_case(&target));
        if !was_following {
            follows.push(target.clone());
        }
        row.contact_pubkeys = serde_json::to_string(&follows).unwrap();
        repo.upsert_in(&tx, &row).await.unwrap();
        if !was_following {
            repo.bump_follower_count_in(&tx, &target, 1).await.unwrap();
        }
        tx.commit().await.unwrap();
        Ok(!was_following)
    })
    .unwrap()
}

/// The regression: two threads following the *same* target concurrently. Both
/// read-modify-write, but each is its own IMMEDIATE transaction, so the second
/// blocks until the first commits and then sees the first's list.
#[test]
fn concurrent_follows_of_the_same_target_do_not_lose_a_write() {
    let (db, path) = temp_db("concurrent");
    let signer = pk(1);
    let target = pk(2);
    seed_signer(&db, &signer);
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO users (pubkey, npub, follower_count) VALUES (?1, '', 0)",
            [target.as_str()],
        ))
        .unwrap();
    }

    let handles: Vec<_> = (0..8)
        .map(|_| {
            // Each thread needs its own Database handle; the pool memoizes on
            // path, so cloning is a refcount bump on the same pool.
            let db = db.clone();
            let (signer, target) = (signer.clone(), target.clone());
            std::thread::spawn(move || follow_in_one_tx(&db, &signer, &target))
        })
        .collect();
    let results: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    assert_eq!(
        results.iter().filter(|b| **b).count(),
        1,
        "exactly one of eight concurrent identical follows should report a new follow, got {results:?}"
    );
    assert_eq!(
        follow_count(&db, &signer, &target),
        1,
        "the target must appear exactly once on the list"
    );

    // And the materialized count moved by exactly +1, not +8.
    let conn = db.conn().unwrap();
    let count: i64 = soshal_db_core::block_on(async {
        let mut r = conn
            .query(
                "SELECT follower_count FROM users WHERE pubkey = ?1",
                [target.as_str()],
            )
            .await
            .unwrap();
        r.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
    });
    assert_eq!(count, 1, "follower_count was bumped once per racer");

    drop(db);
    cleanup(&path);
}

/// The pre-fix shape, as a control: read on one connection, write on another.
///
/// Two threads following *different* targets, with a barrier between the read
/// and the write so the interleaving is forced rather than hoped for. Both see
/// an empty list, both append their own target, and the second write discards
/// the first — a lost follow, and a caller that was told it succeeded.
///
/// This test asserts the loss actually happens, which is what makes the
/// single-transaction test above meaningful: it is the same two writers, and
/// the only difference is where the read sits relative to the write lock.
#[test]
fn the_split_transaction_shape_loses_a_write() {
    let (db, path) = temp_db("split_control");
    let signer = pk(3);
    let a = pk(4);
    let b = pk(5);
    seed_signer(&db, &signer);

    // Force: every reader finishes before any writer starts. This is exactly
    // the window the split shape leaves open, held open on purpose.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

    let handles: Vec<_> = [a.clone(), b.clone()]
        .into_iter()
        .map(|target| {
            let db = db.clone();
            let (signer, target) = (signer.clone(), target);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                // Read with no transaction open, as the old first
                // `with_db_result` did.
                let mut list: Vec<String> = {
                    let row = UserRepo::new(&db).get_by_pubkey(&signer).unwrap().unwrap();
                    serde_json::from_str(&row.contact_pubkeys).unwrap_or_default()
                };
                assert!(
                    list.is_empty(),
                    "the barrier must hold both readers before either writes"
                );
                list.push(target.clone());
                barrier.wait();

                let conn = db.conn().unwrap();
                let repo = UserRepo::new(&db);
                soshal_db_core::query::with_tx(&conn, |tx| async move {
                    let mut row = repo.get_by_pubkey_in(&tx, &signer).await.unwrap().unwrap();
                    row.contact_pubkeys = serde_json::to_string(&list).unwrap();
                    repo.upsert_in(&tx, &row).await.unwrap();
                    tx.commit().await.unwrap();
                    Ok(())
                })
                .unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let survivors: usize = [a.clone(), b.clone()]
        .iter()
        .filter(|t| follow_count(&db, &signer, t) == 1)
        .count();
    assert_eq!(
        survivors, 1,
        "the split-transaction shape loses one of the two follows — that is the bug"
    );

    drop(db);
    cleanup(&path);
}

/// A follow that is already on the list must not double-bump, inside the single
/// transaction. This is the `was_following` gate, and it is what makes the
/// concurrent case above settle on exactly one bump.
#[test]
fn a_repeat_follow_is_a_no_op_for_the_count() {
    let (db, path) = temp_db("repeat");
    let signer = pk(5);
    let target = pk(6);
    seed_signer(&db, &signer);
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO users (pubkey, npub, follower_count) VALUES (?1, '', 0)",
            [target.as_str()],
        ))
        .unwrap();
    }

    assert!(
        follow_in_one_tx(&db, &signer, &target),
        "first follow is new"
    );
    assert!(
        !follow_in_one_tx(&db, &signer, &target),
        "second follow is not new"
    );
    assert!(
        !follow_in_one_tx(&db, &signer, &target),
        "third follow is still not new"
    );

    let conn = db.conn().unwrap();
    let count: i64 = soshal_db_core::block_on(async {
        let mut r = conn
            .query(
                "SELECT follower_count FROM users WHERE pubkey = ?1",
                [target.as_str()],
            )
            .await
            .unwrap();
        r.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
    });
    assert_eq!(count, 1, "three follows must count as one");
    assert_eq!(
        follow_count(&db, &signer, &target),
        1,
        "and list the target once"
    );

    drop(db);
    cleanup(&path);
}

/// The matching unfollow side: only a follow that was actually there is
/// decremented.
#[test]
fn unfollow_decrements_only_when_the_target_was_on_the_list() {
    let (db, path) = temp_db("unfollow");
    let signer = pk(7);
    let target = pk(8);
    seed_signer(&db, &signer);
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute(
            "INSERT INTO users (pubkey, npub, follower_count) VALUES (?1, '', 0)",
            [target.as_str()],
        ))
        .unwrap();
    }

    let unfollow = |db: &Database| -> bool {
        let conn = db.conn().unwrap();
        let repo = UserRepo::new(db);
        let (signer, target) = (signer.clone(), target.clone());
        soshal_db_core::query::with_tx(&conn, |tx| async move {
            let Some(mut row) = repo.get_by_pubkey_in(&tx, &signer).await.unwrap() else {
                return Ok(false);
            };
            let mut follows: Vec<String> =
                serde_json::from_str(&row.contact_pubkeys).unwrap_or_default();
            let was_following = follows.iter().any(|f| f.eq_ignore_ascii_case(&target));
            follows.retain(|f| !f.eq_ignore_ascii_case(&target));
            row.contact_pubkeys = serde_json::to_string(&follows).unwrap();
            repo.upsert_in(&tx, &row).await.unwrap();
            if was_following {
                repo.bump_follower_count_in(&tx, &target, -1).await.unwrap();
            }
            tx.commit().await.unwrap();
            Ok(was_following)
        })
        .unwrap()
    };

    let count = |db: &Database| -> i64 {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(async {
            let mut r = conn
                .query(
                    "SELECT follower_count FROM users WHERE pubkey = ?1",
                    [target.as_str()],
                )
                .await
                .unwrap();
            r.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
        })
    };

    follow_in_one_tx(&db, &signer, &target);
    assert_eq!(count(&db), 1);

    assert!(unfollow(&db), "first unfollow finds the target");
    assert_eq!(count(&db), 0, "unfollow decremented once");

    assert!(!unfollow(&db), "second unfollow finds nothing");
    assert_eq!(count(&db), 0, "and must not decrement again");

    drop(db);
    cleanup(&path);
}
