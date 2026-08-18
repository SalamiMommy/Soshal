//! Migration 3: reactions on group threads and thread replies.

use libsql::Connection;

pub fn v3_group_thread_reactions(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS group_thread_reactions (
            thread_id TEXT NOT NULL DEFAULT '',
            reply_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (thread_id, reply_id, pubkey, emoji)
        );

        CREATE INDEX IF NOT EXISTS idx_gtr_thread ON group_thread_reactions(thread_id);
        CREATE INDEX IF NOT EXISTS idx_gtr_reply ON group_thread_reactions(reply_id);

        INSERT OR IGNORE INTO _migrations (version) VALUES (3);
        ",
    ))?;
    Ok(())
}
