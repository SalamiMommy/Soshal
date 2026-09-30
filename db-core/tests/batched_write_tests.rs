//! Equivalence of the batched write paths against the per-row loops they
//! replaced: `bump_follower_counts_in` and `upsert_many_in`.
//!
//! The batching is a performance change to a correctness-sensitive area
//! (materialized follower counts, trend counts that must not inflate on relay
//! replay), so each test compares the batch against the sequential form it
//! replaces rather than asserting a hand-computed expectation. If the two ever
//! disagree, the batch is wrong.
//!
//! `with_tx` is driven through `block_on` rather than a helper, matching the
//! rest of this crate's tests: its closure returns a future that borrows the
//! transaction, so a generic wrapper cannot be written without HRTB trouble.

use soshal_db_core::repos::hashtag::{HashtagRepo, HashtagRow};
use soshal_db_core::repos::user::UserRepo;
use soshal_db_core::Database;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_db(tag: &str) -> (Database, String) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
        .join(format!(
            "soshal_batched_{tag}_{}_{n}.db",
            std::process::id()
        ))
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

fn seed_users(db: &Database, keys: &[String]) {
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(async {
        for k in keys {
            conn.execute(
                "INSERT INTO users (pubkey, npub, follower_count) VALUES (?1, '', 0)",
                [k.as_str()],
            )
            .await
            .unwrap();
        }
    });
}

fn follower_count(db: &Database, key: &str) -> i64 {
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(async {
        let mut r = conn
            .query("SELECT follower_count FROM users WHERE pubkey = ?1", [key])
            .await
            .unwrap();
        match r.next().await.unwrap() {
            Some(row) => row.get::<i64>(0).unwrap(),
            None => panic!("no such user {key}"),
        }
    })
}

fn hashtag_rows(db: &Database) -> Vec<(String, String, i64, i64)> {
    let conn = db.conn().unwrap();
    soshal_db_core::block_on(async {
        let mut r = conn
            .query(
                "SELECT tag, pubkey, last_used_at, count FROM hashtags ORDER BY tag, pubkey",
                (),
            )
            .await
            .unwrap();
        let mut out = Vec::new();
        while let Some(row) = r.next().await.unwrap() {
            out.push((
                row.get::<String>(0).unwrap(),
                row.get::<String>(1).unwrap(),
                row.get::<i64>(2).unwrap(),
                row.get::<i64>(3).unwrap(),
            ));
        }
        out
    })
}

/// Every case, applied through the per-pubkey loop and through the batch, with
/// the resulting counts compared. Cases are named so a failure says which shape
/// diverged.
#[test]
fn batched_follower_deltas_match_the_per_pubkey_loop() {
    let keys: Vec<String> = (1..=8).map(pk).collect();
    let cases: [(&str, Vec<(&str, i64)>); 5] = [
        (
            "all adds",
            vec![
                (&keys[0], 1),
                (&keys[1], 1),
                (&keys[2], 1),
                (&keys[3], 1),
                (&keys[4], 1),
            ],
        ),
        (
            "adds and removes together",
            vec![(&keys[0], 1), (&keys[1], -1), (&keys[2], 1), (&keys[3], -1)],
        ),
        (
            "added and removed in the same batch",
            vec![(&keys[4], 1), (&keys[4], -1), (&keys[5], 1)],
        ),
        (
            "mixed magnitudes",
            vec![(&keys[6], 5), (&keys[7], -2), (&keys[0], 3)],
        ),
        ("empty", vec![]),
    ];

    for (label, deltas) in cases {
        let (db_seq, path_seq) = temp_db(&format!("follower_seq_{}", keys.len()));
        let (db_bat, path_bat) = temp_db(&format!("follower_bat_{}", keys.len()));
        seed_users(&db_seq, &keys);
        seed_users(&db_bat, &keys);

        {
            let conn = db_seq.conn().unwrap();
            let repo = UserRepo::new(&db_seq);
            let deltas = deltas.clone();
            soshal_db_core::query::with_tx(&conn, |t| async move {
                for (k, d) in deltas {
                    repo.bump_follower_count_in(&t, k, d).await.unwrap();
                }
                t.commit().await.unwrap();
                Ok(())
            })
            .unwrap();
        }
        {
            let conn = db_bat.conn().unwrap();
            let repo = UserRepo::new(&db_bat);
            let deltas = deltas.clone();
            soshal_db_core::query::with_tx(&conn, |t| async move {
                repo.bump_follower_counts_in(&t, &deltas).await.unwrap();
                t.commit().await.unwrap();
                Ok(())
            })
            .unwrap();
        }

        for k in &keys {
            assert_eq!(
                follower_count(&db_seq, k),
                follower_count(&db_bat, k),
                "case {label:?}: batched path diverged from the sequential loop at {k}"
            );
        }

        drop(db_seq);
        drop(db_bat);
        cleanup(&path_seq);
        cleanup(&path_bat);
    }
}

/// The clamp is the subtle part: `MAX(0, count + delta)` must hold, and a
/// removed-follower pass over a zeroed row must not drive it negative.
#[test]
fn batched_deltas_still_clamp_at_zero() {
    let keys: Vec<String> = (1..=3).map(pk).collect();
    let (db, path) = temp_db("clamp");
    seed_users(&db, &keys);

    {
        let conn = db.conn().unwrap();
        let repo = UserRepo::new(&db);
        let deltas: Vec<(&str, i64)> = keys.iter().map(|k| (k.as_str(), -5i64)).collect();
        soshal_db_core::query::with_tx(&conn, |t| async move {
            repo.bump_follower_counts_in(&t, &deltas).await.unwrap();
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }

    for k in &keys {
        assert_eq!(
            follower_count(&db, k),
            0,
            "a -5 against a 0 count must clamp, not go negative"
        );
    }

    drop(db);
    cleanup(&path);
}

/// `WHERE pubkey IN (…)` must not touch rows outside the batch.
#[test]
fn batched_deltas_leave_unlisted_rows_alone() {
    let keys: Vec<String> = (1..=4).map(pk).collect();
    let (db, path) = temp_db("scoped");
    seed_users(&db, &keys);
    // Give an unlisted row a non-zero count so a stray UPDATE would be visible.
    {
        let conn = db.conn().unwrap();
        soshal_db_core::block_on(conn.execute(
            "UPDATE users SET follower_count = 42 WHERE pubkey = ?1",
            [keys[3].as_str()],
        ))
        .unwrap();
    }

    {
        let conn = db.conn().unwrap();
        let repo = UserRepo::new(&db);
        let deltas: Vec<(&str, i64)> = vec![(keys[0].as_str(), 1), (keys[1].as_str(), 1)];
        soshal_db_core::query::with_tx(&conn, |t| async move {
            repo.bump_follower_counts_in(&t, &deltas).await.unwrap();
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }

    assert_eq!(
        follower_count(&db, &keys[3]),
        42,
        "row 3 was not in the batch"
    );
    assert_eq!(follower_count(&db, &keys[0]), 1);
    assert_eq!(follower_count(&db, &keys[1]), 1);
    assert_eq!(
        follower_count(&db, &keys[2]),
        0,
        "row 2 was not in the batch"
    );

    drop(db);
    cleanup(&path);
}

fn tag(tag: &str, pubkey: &str, last_used_at: i64) -> HashtagRow {
    HashtagRow {
        tag: tag.to_string(),
        pubkey: pubkey.to_string(),
        last_used_at,
        count: 1,
    }
}

/// The trend-count guard is the reason the conflict clause looks the way it
/// does: `count` bumps only for a *strictly newer* usage, so a relay replay of
/// an already-seen post (same timestamp) cannot inflate it. Batching must not
/// change that, which is exactly what a naive rewrite would.
#[test]
fn batched_hashtag_upsert_matches_the_per_tag_loop() {
    // Built as closures rather than as a `Vec` of cases: `HashtagRow` is not
    // `Clone`, and each case needs its own copy for each of the two paths.
    type TagCase = (&'static str, Box<dyn Fn() -> Vec<HashtagRow>>);
    let cases: Vec<TagCase> = vec![
        (
            "distinct tags, one post",
            Box::new(|| {
                let k = pk(1);
                vec![tag("a", &k, 100), tag("b", &k, 100), tag("c", &k, 100)]
            }),
        ),
        (
            "same tag/pubkey/timestamp (a relay replay)",
            Box::new(|| {
                let k = pk(2);
                vec![tag("x", &k, 200), tag("x", &k, 200)]
            }),
        ),
        (
            "same tag/pubkey, strictly newer",
            Box::new(|| {
                let k = pk(3);
                vec![tag("y", &k, 300), tag("y", &k, 400)]
            }),
        ),
        (
            "several authors, same tag",
            Box::new(|| vec![tag("z", &pk(4), 500), tag("z", &pk(5), 500)]),
        ),
        ("empty", Box::new(Vec::new)),
    ];

    for (label, build) in cases {
        let (db_seq, path_seq) = temp_db("tag_seq");
        let (db_bat, path_bat) = temp_db("tag_bat");

        {
            let conn = db_seq.conn().unwrap();
            let repo = HashtagRepo::new(&db_seq);
            let rows = build();
            soshal_db_core::query::with_tx(&conn, |t| async move {
                for row in &rows {
                    repo.upsert_in(&t, row).await.unwrap();
                }
                t.commit().await.unwrap();
                Ok(())
            })
            .unwrap();
        }
        {
            let conn = db_bat.conn().unwrap();
            let repo = HashtagRepo::new(&db_bat);
            let rows = build();
            soshal_db_core::query::with_tx(&conn, |t| async move {
                repo.upsert_many_in(&t, &rows).await.unwrap();
                t.commit().await.unwrap();
                Ok(())
            })
            .unwrap();
        }

        assert_eq!(
            hashtag_rows(&db_seq),
            hashtag_rows(&db_bat),
            "case {label:?}: batched hashtag upsert diverged from the per-tag loop"
        );

        drop(db_seq);
        drop(db_bat);
        cleanup(&path_seq);
        cleanup(&path_bat);
    }
}

/// Spelled out because it is the regression the guard exists to prevent: a
/// replayed post must leave the trend count alone, through the batched path.
#[test]
fn batched_hashtag_upsert_does_not_inflate_on_replay() {
    let (db, path) = temp_db("replay");
    let k = pk(9);

    {
        let conn = db.conn().unwrap();
        let repo = HashtagRepo::new(&db);
        let rows = vec![tag("news", &k, 900)];
        soshal_db_core::query::with_tx(&conn, |t| async move {
            repo.upsert_many_in(&t, &rows).await.unwrap();
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(hashtag_rows(&db)[0].3, 1);

    // Same timestamp again — a relay replay. Count must stay 1.
    {
        let conn = db.conn().unwrap();
        let repo = HashtagRepo::new(&db);
        let rows = vec![tag("news", &k, 900)];
        soshal_db_core::query::with_tx(&conn, |t| async move {
            repo.upsert_many_in(&t, &rows).await.unwrap();
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(hashtag_rows(&db)[0].3, 1, "replay inflated the trend count");

    // A strictly newer post does bump it, and advances last_used_at.
    {
        let conn = db.conn().unwrap();
        let repo = HashtagRepo::new(&db);
        let rows = vec![tag("news", &k, 901)];
        soshal_db_core::query::with_tx(&conn, |t| async move {
            repo.upsert_many_in(&t, &rows).await.unwrap();
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }
    let got = hashtag_rows(&db);
    assert_eq!(got[0].3, 2, "a newer usage must bump the count");
    assert_eq!(got[0].2, 901, "last_used_at must advance");

    drop(db);
    cleanup(&path);
}

/// A multi-row upsert's conflict clause sees earlier rows *from the same
/// statement*, so the batch is exactly equivalent to N single-row calls — not
/// just equivalent on the shapes ingest happens to produce.
///
/// This was written as a throwaway probe while documenting
/// `HashtagRepo::upsert_many_in`, and it disproved the mechanism the first
/// draft of that doc claimed. Kept because it is a real equivalence property
/// that the caller relies on, and because "SQLite applies the conflict clause
/// against the table as it was when the statement started" is a plausible-
/// sounding falsehood that a future refactor could reintroduce.
///
/// Ingest never actually emits this shape — it dedups tags per post — so
/// nothing in the app depends on it. It is pinned because the doc claims it.
#[test]
fn batched_hashtag_upsert_chains_strictly_increasing_rows() {
    let (db_seq, path_seq) = temp_db("tag_chain_seq");
    let (db_bat, path_bat) = temp_db("tag_chain_bat");
    let k = pk(9);
    seed_users(&db_seq, &[k.clone()]);
    seed_users(&db_bat, &[k.clone()]);

    let rows = || vec![tag("p", &k, 100), tag("p", &k, 200), tag("p", &k, 300)];

    {
        let conn = db_seq.conn().unwrap();
        let repo = HashtagRepo::new(&db_seq);
        let rows = rows();
        soshal_db_core::query::with_tx(&conn, |t| async move {
            for r in &rows {
                repo.upsert_in(&t, r).await.unwrap();
            }
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }
    {
        let conn = db_bat.conn().unwrap();
        let repo = HashtagRepo::new(&db_bat);
        let rows = rows();
        soshal_db_core::query::with_tx(&conn, |t| async move {
            repo.upsert_many_in(&t, &rows).await.unwrap();
            t.commit().await.unwrap();
            Ok(())
        })
        .unwrap();
    }

    // If the conflict clause did NOT see earlier rows, the batched run would
    // land count=1 (all three rows see the pre-statement state and the guard
    // lets only the first insert through), so this asserts count=3 twice over.
    assert_eq!(hashtag_rows(&db_bat), hashtag_rows(&db_seq));
    assert_eq!(hashtag_rows(&db_bat)[0].3, 3, "all three usages must count");

    drop(db_seq);
    drop(db_bat);
    cleanup(&path_seq);
    cleanup(&path_bat);
}
