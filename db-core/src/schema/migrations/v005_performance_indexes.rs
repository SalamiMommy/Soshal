//! Migration 5: performance indexes and zk_state_rollups table.

use libsql::Connection;

pub fn v5_performance_indexes(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        -- Poll vote lookups
        CREATE UNIQUE INDEX IF NOT EXISTS idx_poll_votes_poll_voter ON poll_votes(poll_id, voter_pubkey);
        CREATE INDEX IF NOT EXISTS idx_poll_votes_poll_option ON poll_votes(poll_id, option_id);

        -- Group member counts and queries
        CREATE INDEX IF NOT EXISTS idx_group_members_group ON group_members(group_id);

        -- Thread reply trees
        CREATE INDEX IF NOT EXISTS idx_group_thread_replies_parent ON group_thread_replies(parent_id, created_at ASC);

        -- Reaction listings
        CREATE INDEX IF NOT EXISTS idx_reactions_event_created ON reactions(event_id, created_at DESC);

        -- Filtered unread notifications
        CREATE INDEX IF NOT EXISTS idx_notifications_filter ON notifications(pubkey, type, is_read, created_at DESC);

        -- Musicloud comments
        CREATE INDEX IF NOT EXISTS idx_musicloud_comments_track ON musicloud_comments(track_id, created_at ASC);

        -- Guestbook approvals
        CREATE INDEX IF NOT EXISTS idx_guestbook_entries_approved ON guestbook_entries(profile_pubkey, approved, created_at DESC);

        -- Stream chat by user
        CREATE INDEX IF NOT EXISTS idx_stream_chat_pubkey ON stream_chat(pubkey, created_at DESC);

        -- ZK state rollups persistence
        CREATE TABLE IF NOT EXISTS zk_state_rollups (
            thread_id TEXT PRIMARY KEY,
            genesis_root TEXT NOT NULL,
            final_state_root TEXT NOT NULL,
            operation_count INTEGER NOT NULL,
            verified_at INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO _migrations (version) VALUES (5);
        ",
    ))?;
    Ok(())
}
