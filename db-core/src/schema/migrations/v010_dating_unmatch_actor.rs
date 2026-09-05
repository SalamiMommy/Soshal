//! Migration 10: scope `dating_unmatches` by actor. The table previously
//! keyed only by the target pubkey, so an unmatch recorded under one
//! account hid that profile from every other account on the device.
//!
//! SQLite cannot ALTER a table to change its PRIMARY KEY, so the table is
//! rebuilt with a composite `(actor_pubkey, pubkey)` key. Pre-existing rows
//! have no actor — they are dropped (transient: a stale swipe-suppression
//! entry is harmless and re-appears on the next unmatch).

use libsql::Connection;

pub fn create_dating_unmatch_actor_column(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS dating_unmatches_new (
            actor_pubkey TEXT NOT NULL,
            pubkey TEXT NOT NULL,
            unmatched_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (actor_pubkey, pubkey)
        );

        DROP TABLE IF EXISTS dating_unmatches;
        ALTER TABLE dating_unmatches_new RENAME TO dating_unmatches;

INSERT OR IGNORE INTO _migrations (version) VALUES (10);
        ",
    ))?;
    Ok(())
}
