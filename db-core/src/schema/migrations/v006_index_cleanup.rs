//! Migration 6: index cleanup (drop redundant prefixes and duplicates) + feed cursor index.

use libsql::Connection;

pub fn v6_index_cleanup(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        -- Drop duplicate / redundant prefix indexes
        DROP INDEX IF EXISTS idx_posts_pubkey;
        DROP INDEX IF EXISTS idx_messages_conv;
        DROP INDEX IF EXISTS idx_bookmarks_pubkey;
        DROP INDEX IF EXISTS idx_media_pubkey;
        DROP INDEX IF EXISTS idx_audit_logs_actor;
        DROP INDEX IF EXISTS idx_notifications_unread;

        -- Cursor-based covering index for keyset feed pagination
        CREATE INDEX IF NOT EXISTS idx_posts_feed_cursor ON posts(pubkey, is_deleted, created_at DESC, id);
        CREATE INDEX IF NOT EXISTS idx_posts_recent_cursor ON posts(is_deleted, created_at DESC, id);

        INSERT OR IGNORE INTO _migrations (version) VALUES (6);
        ",
    ))?;
    Ok(())
}
