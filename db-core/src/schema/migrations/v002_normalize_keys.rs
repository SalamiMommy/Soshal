//! Migration 2: normalize key columns to lowercase.
//!
//! # Why
//!
//! The query layer matches key-shaped columns case-insensitively with
//! `LOWER(col) = LOWER(?)`. Wrapping an indexed column in a function makes its
//! btree unusable, so every one of those predicates is a full table scan
//! instead of a primary-key seek. That is a large share of the ~310 `LOWER()`
//! call sites in the query layer.
//!
//! `LOWER()` can only be dropped once the *stored* value is guaranteed
//! lowercase, so this migration backfills any pre-existing mixed-case rows
//! after the write paths were fixed to normalize (see
//! `db-core/tests/key_normalization_tests.rs` for the invariant).
//!
//! # How
//!
//! Two passes per column, in this order:
//!
//! 1. **Dedupe.** `UPDATE`ing a primary key into a value that already exists
//!    aborts the statement, so colliding rows must go first. Dedupe groups on
//!    the table's *full* primary key with each key column normalized, not on
//!    one column at a time — for a composite PK like
//!    `ignored_notifications(pubkey, kind, from_pubkey, event_id)`, grouping
//!    on `pubkey` alone would delete legitimately distinct rows. The surviving
//!    row is the highest `rowid`, i.e. the most recently written of the
//!    duplicates.
//! 2. **Normalize.** `UPDATE ... SET col = LOWER(TRIM(col))` for rows that are
//!    not already normalized.
//!
//! # Foreign keys
//!
//! `PRAGMA foreign_keys` is a documented no-op inside a transaction, and every
//! migration step runs inside the `BEGIN IMMEDIATE` that `migrate()` opens, so
//! FKs cannot simply be switched off. `PRAGMA defer_foreign_keys=ON` *does*
//! work inside a transaction: it defers every FK check to COMMIT. That is what
//! lets a child column be normalized before its parent.
//!
//! The deferral doubles as the correctness check — if this list ever covers a
//! child whose parent is left mixed-case, COMMIT fails and the whole migration
//! rolls back rather than silently orphaning rows.
//!
//! # Not covered
//!
//! The `LOWER(value)` sites are `json_each` elements, not columns: JSON arrays
//! of pubkeys (`contact_pubkeys`, `relay_list`, `mentioned_pubkeys`) hold
//! embedded keys that a column `UPDATE` cannot reach without rewriting the
//! document. Those keep their `LOWER()` until a separate migration does the
//! JSON rewrite.

use libsql::Connection;

/// Tables whose key columns get normalized, paired with the columns to
/// normalize. Deliberately an explicit list rather than "every column named
/// `pubkey`" so that adding a case-sensitive column is a conscious decision.
const TARGETS: &[(&str, &[&str])] = &[
    ("users", &["pubkey"]),
    ("posts", &["id", "pubkey", "root_id"]),
    ("reactions", &["id", "pubkey", "event_id"]),
    ("reposts", &["id", "pubkey", "event_id"]),
    ("bookmarks", &["id", "pubkey", "event_id"]),
    (
        "zaps",
        &[
            "id",
            "pubkey",
            "sender_pubkey",
            "recipient_pubkey",
            "event_id",
        ],
    ),
    ("blocks", &["pubkey", "blocked_pubkey"]),
    (
        "notifications",
        &["id", "pubkey", "from_pubkey", "event_id"],
    ),
    (
        "ignored_notifications",
        &["pubkey", "from_pubkey", "event_id"],
    ),
    ("saved_content", &["id", "pubkey"]),
    ("guestbook_entries", &["id", "profile_pubkey"]),
    ("messages", &["id", "pubkey"]),
    ("escrows", &["buyer_pubkey", "seller_pubkey"]),
    ("friend_backups", &["user_pubkey"]),
    ("spam_reports", &["pubkey", "target_pubkey"]),
    ("poll_votes", &["voter_pubkey"]),
    ("polls", &["pubkey"]),
    ("post_views", &["pubkey", "post_id"]),
    ("hashtags", &["pubkey"]),
    ("story_reactions", &["pubkey"]),
    ("groups", &["pubkey"]),
    ("group_members", &["pubkey"]),
    ("banned_members", &["pubkey"]),
    ("group_join_requests", &["pubkey"]),
    ("group_voice_presence", &["pubkey"]),
    ("group_thread_reactions", &["pubkey"]),
    ("group_room_reactions", &["pubkey"]),
    ("musiclouds", &["pubkey"]),
    ("musicloud_comments", &["pubkey"]),
    ("relays", &["pubkey"]),
    ("dating_unmatches", &["actor_pubkey", "pubkey"]),
    ("secret_crushes", &["owner_pubkey", "crush_pubkey"]),
    ("marketplace_saved", &["pubkey"]),
    ("tx_edges", &["parent_id", "child_id"]),
];

/// Quote an identifier for interpolation. Identifiers here are compile-time
/// constants from `TARGETS`, so this is belt-and-braces, not a user-input
/// guard — but `trusted_schema=OFF` and `secure_delete=ON` mean the DB is
/// already hardened and a stray quote should not be the thing that breaks it.
fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Primary-key columns of `table`, in key order, as reported by SQLite.
/// Read at runtime so the dedupe grouping follows the schema instead of a
/// hand-maintained copy that could drift.
fn pk_columns(conn: &Connection, table: &str) -> Result<Vec<String>, libsql::Error> {
    crate::block_on(async {
        let mut rows = conn
            .query(&format!("PRAGMA table_info({})", q(table)), ())
            .await?;
        let mut pks: Vec<(i64, String)> = Vec::new();
        while let Some(row) = rows.next().await? {
            // table_info columns: cid, name, type, notnull, dflt_value, pk
            let name: String = row.get(1)?;
            let pk_pos: i64 = row.get(5)?;
            if pk_pos > 0 {
                pks.push((pk_pos, name));
            }
        }
        pks.sort_by_key(|(pos, _)| *pos);
        Ok(pks.into_iter().map(|(_, name)| name).collect())
    })
}

/// The migration body, run between `SAVEPOINT` and `RELEASE`.
fn normalize_all(conn: &Connection) -> Result<(), libsql::Error> {
    // See module docs: this is what makes a parents-after-children ordering
    // safe, and it turns a missed parent into a COMMIT-time failure rather
    // than an orphaned row.
    crate::block_on(conn.execute_batch("PRAGMA defer_foreign_keys=ON"))?;

    for &(table, cols) in TARGETS {
        let pk = pk_columns(conn, table)?;
        if pk.is_empty() {
            // No primary key to collide on, so no dedupe is needed. Every
            // table in TARGETS has one; this is a guard, not an expected path.
            continue;
        }

        // Group by the full primary key, normalizing the columns this
        // migration rewrites and passing the rest through untouched.
        let group_exprs: Vec<String> = pk
            .iter()
            .map(|c| {
                if cols.contains(&c.as_str()) {
                    format!("LOWER(TRIM({}))", q(c))
                } else {
                    q(c)
                }
            })
            .collect();

        // Pass 1: collapse rows that would collide on the normalized key.
        // Highest rowid wins (most recently written).
        crate::block_on(conn.execute(
            &format!(
                "DELETE FROM {t} WHERE rowid NOT IN \
                 (SELECT MAX(rowid) FROM {t} GROUP BY {groups})",
                t = q(table),
                groups = group_exprs.join(", "),
            ),
            (),
        ))?;

        // Pass 2: normalize. The WHERE keeps this a no-op for rows that are
        // already correct, which is the overwhelmingly common case.
        for &col in cols {
            crate::block_on(conn.execute(
                &format!(
                    "UPDATE {t} SET {c} = LOWER(TRIM({c})) \
                     WHERE {c} <> LOWER(TRIM({c}))",
                    t = q(table),
                    c = q(col),
                ),
                (),
            ))?;
        }
    }

    crate::block_on(conn.execute_batch("INSERT OR IGNORE INTO _migrations (version) VALUES (2);"))?;
    Ok(())
}

/// Backfill key columns to lowercase.
///
/// A transaction is required — normalizing a parent while a child still holds
/// the old mixed-case key violates the child's foreign key, and
/// `defer_foreign_keys` only defers the check to COMMIT. `SAVEPOINT` gives us
/// one in both call shapes: it starts a transaction when none is active, and
/// nests harmlessly inside the `BEGIN IMMEDIATE` that `migrate()` opens around
/// its steps. Either way the step is atomic on its own.
pub fn v2_normalize_keys(conn: &Connection) -> Result<(), libsql::Error> {
    const SP: &str = "SAVEPOINT v2_normalize_keys";
    crate::block_on(conn.execute_batch(SP))?;
    match normalize_all(conn) {
        Ok(()) => {
            crate::block_on(conn.execute_batch("RELEASE v2_normalize_keys"))?;
            Ok(())
        }
        Err(e) => {
            let _ = crate::block_on(conn.execute_batch("ROLLBACK TO v2_normalize_keys"));
            let _ = crate::block_on(conn.execute_batch("RELEASE v2_normalize_keys"));
            Err(e)
        }
    }
}
