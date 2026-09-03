//! Migration 9: optimize posts_au FTS trigger to only fire when content,
//! subject, or deletion status changes, eliminating write amplification and
//! full FTS5 re-tokenization on post metadata updates (sync_status, reactions, etc.).

use libsql::Connection;

pub fn v9_trigger_optimization(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        DROP TRIGGER IF EXISTS posts_au;

        CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE OF content, subject, is_deleted ON posts
        WHEN old.content != new.content OR old.subject IS NOT new.subject OR old.is_deleted != new.is_deleted
        BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject)
            SELECT new.rowid, new.id, new.pubkey, new.content, new.subject WHERE new.is_deleted = 0;
        END;

        INSERT OR IGNORE INTO _migrations (version) VALUES (9);
        ",
    ))?;
    Ok(())
}
