//! Migration 8: drop duplicate / redundant indexes (write amplification on
//! the hottest tables) and add the missing escrow listing index.

use libsql::Connection;

pub fn v8_index_cleanup(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        -- Exact duplicate of v007 idx_posts_user_timeline
        DROP INDEX IF EXISTS idx_posts_feed_lookup;

        -- Prefix of v006 idx_posts_recent_cursor
        DROP INDEX IF EXISTS idx_posts_recent_lookup;

        -- Prefix of idx_posts_kind_deleted_created (feed queries always filter is_deleted)
        DROP INDEX IF EXISTS idx_posts_kind_created;

        -- Duplicate of v005 idx_notifications_filter (same cols, different order)
        DROP INDEX IF EXISTS idx_notifications_unread_type;

        -- Same columns as v007 idx_messages_conversation_asc, opposite sort
        -- (SQLite walks either index backward)
        DROP INDEX IF EXISTS idx_messages_conv_deleted;

        -- Escrow listing ordered by created_at (full scan + sort today)
        CREATE INDEX IF NOT EXISTS idx_escrows_created ON escrows(created_at DESC);

        INSERT OR IGNORE INTO _migrations (version) VALUES (8);
        ",
    ))?;
    Ok(())
}
