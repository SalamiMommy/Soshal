//! Migration 3: indexes for the reverse direction of composite key lookups.
//!
//! # Why
//!
//! Migrations 1 and 2 made the key columns lowercase, which is what let the
//! `LOWER()` wrappers come off every predicate in `db-core/src/repos` and
//! `flutter-bridge/src/ffi`. Four of those predicates turned out to have no
//! index on the far side of their lookup, so dropping the wrapper changed a
//! *folded* scan into an unfolded one rather than into a seek. Verified with
//! `EXPLAIN QUERY PLAN` before and after this migration; the regression tests
//! live in `db-core/tests/key_normalization_tests.rs`.
//!
//! Two kinds of gap:
//!
//! * **Composite-PK suffixes.** `dating_unmatches(actor_pubkey, pubkey)` and
//!   `blocks(pubkey, blocked_pubkey)` are keyed in one direction only. The
//!   dating discovery query asks both directions — "who did I unmatch" *and*
//!   "who unmatch-blocked me" — so one of the two always scanned.
//! * **Unindexed columns.** `musicloud_playlists.pubkey` and
//!   `saved_content.pubkey` had no index at all; their reads filter on the
//!   author, which the primary key does not cover.
//!
//! # How
//!
//! `CREATE INDEX IF NOT EXISTS`, so re-running is a no-op. All four are
//! secondary indexes over a subset of rows per key, so they cost disk but not
//! write amplification beyond the existing index rate — the app already
//! maintains a comparable number of them.
//!
//! # Ordering
//!
//! No data rewrite, so this ordering-independently safe to run before or after
//! v2. It is registered after it because it exists to serve the predicates v2
//! unblocked.

use libsql::Connection;

/// `(table, column, index name)` for each gap. The index name matches the
/// `idx_<table>_<column>` convention the rest of the schema uses.
const INDEXES: &[(&str, &str, &str)] = &[
    // PK is (actor_pubkey, pubkey); this covers the reverse direction.
    ("dating_unmatches", "pubkey", "idx_dating_unmatches_pubkey"),
    // PK is (pubkey, blocked_pubkey); this covers the reverse direction.
    ("blocks", "blocked_pubkey", "idx_blocks_blocked_pubkey"),
    // Author listings with no index on the author at all.
    ("musicloud_playlists", "pubkey", "idx_mcl_pl_pubkey"),
    ("saved_content", "pubkey", "idx_saved_content_pubkey"),
];

fn table_exists(conn: &Connection, table: &str) -> Result<bool, libsql::Error> {
    crate::block_on(async {
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(row.get::<i64>(0)? > 0),
            None => Ok(false),
        }
    })
}

/// The migration body, run between `SAVEPOINT` and `RELEASE`.
fn create_all(conn: &Connection) -> Result<(), libsql::Error> {
    for &(table, column, index) in INDEXES {
        // `CREATE INDEX IF NOT EXISTS` still fails when the *table* is absent,
        // and `migrate()` has to tolerate the partial legacy schemas that
        // `heal_legacy_schema` exists to repair — a pre-squash database can be
        // missing tables this step would otherwise assume. v2 skips absent
        // tables the same way (an empty `PRAGMA table_info` yields no primary
        // key to normalize).
        if !table_exists(conn, table)? {
            continue;
        }
        crate::block_on(conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS {index} ON {table}({column})",
                index = super::quote_ident(index),
                table = super::quote_ident(table),
                column = super::quote_ident(column),
            ),
            (),
        ))?;
    }

    crate::block_on(conn.execute_batch("INSERT OR IGNORE INTO _migrations (version) VALUES (3);"))?;
    Ok(())
}

/// Create the reverse-lookup indexes.
///
/// `SAVEPOINT` mirrors `v2_normalize_keys`: it opens a transaction when none is
/// active and nests harmlessly inside the `BEGIN IMMEDIATE` that `migrate()`
/// wraps its steps in. Index creation is transactional in SQLite, so a failure
/// part-way leaves none of them behind.
pub fn v3_reverse_key_indexes(conn: &Connection) -> Result<(), libsql::Error> {
    const SP: &str = "SAVEPOINT v3_reverse_key_indexes";
    crate::block_on(conn.execute_batch(SP))?;
    match create_all(conn) {
        Ok(()) => {
            crate::block_on(conn.execute_batch("RELEASE v3_reverse_key_indexes"))?;
            Ok(())
        }
        Err(e) => {
            let _ = crate::block_on(conn.execute_batch("ROLLBACK TO v3_reverse_key_indexes"));
            let _ = crate::block_on(conn.execute_batch("RELEASE v3_reverse_key_indexes"));
            Err(e)
        }
    }
}
