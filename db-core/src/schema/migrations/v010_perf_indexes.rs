//! Migration 10: covering indexes for hot query patterns (group message
//! fetch, outbox retry sweeps, conversation fetch, reminder ticks, geohash
//! peer pruning, huddle expiry cleanup, diagnostic log pruning).

use libsql::Connection;

pub fn v10_perf_indexes(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_group_messages_fetch ON group_messages(group_id, is_deleted, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_outbox_queue_pending ON outbox_queue(status, next_retry_at);

        CREATE INDEX IF NOT EXISTS idx_outbox_queue_created ON outbox_queue(status, created_at);

        CREATE INDEX IF NOT EXISTS idx_messages_conv_deleted ON messages(conversation_id, is_deleted, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_reminders_start ON reminders(start_time);

        CREATE INDEX IF NOT EXISTS idx_geohash_peers_last_seen ON geohash_peers(last_seen);

        CREATE INDEX IF NOT EXISTS idx_huddle_posts_expires ON huddle_posts(expires_at);

        CREATE INDEX IF NOT EXISTS idx_diagnostic_logs_created ON diagnostic_logs(created_at);

        INSERT OR IGNORE INTO _migrations (version) VALUES (10);
    ",
    ))?;
    Ok(())
}
