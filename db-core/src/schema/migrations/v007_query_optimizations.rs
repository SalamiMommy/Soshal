//! Migration 7: query performance indexes for user timeline feed queries,
//! chronological message lookups, and fast reaction/repost existence checks.

use libsql::Connection;

pub fn v7_query_optimizations(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        -- Fast user profile feed timeline lookup (skipping deleted posts)
        CREATE INDEX IF NOT EXISTS idx_posts_user_timeline ON posts(pubkey, is_deleted, created_at DESC);

        -- Chronological chat message pagination
        CREATE INDEX IF NOT EXISTS idx_messages_conversation_asc ON messages(conversation_id, is_deleted, created_at ASC);

        -- Fast existence check for user reactions on events
        CREATE INDEX IF NOT EXISTS idx_reactions_event_pubkey ON reactions(event_id, pubkey);

        -- Fast existence check for user reposts on events
        CREATE INDEX IF NOT EXISTS idx_reposts_event_pubkey ON reposts(event_id, pubkey);

        INSERT OR IGNORE INTO _migrations (version) VALUES (7);
        ",
    ))?;
    Ok(())
}
