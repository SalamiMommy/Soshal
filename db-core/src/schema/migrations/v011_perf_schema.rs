//! Migration 11: perf schema for round-4 sweep.
//!
//! - `conversations` table (inbox = ORDER BY last_message_at instead of a
//!   full scan + GROUP BY + sort over every message row).
//! - `posts.rsvp_event_id` (kind-31924 e-tag denormalized at ingest) so RSVP
//!   queries hit an index instead of a `tags_json LIKE` scan.
//! - `reminders.trigger_at` (start_time - minutes_before*60 precomputed) so
//!   the due-window query uses an index instead of an expression filter.
//! - covering indexes for thread fetch, notification tabs, feed paging,
//!   audit-log pruning and geohash peer sweep.

use libsql::Connection;

pub fn v11_perf_schema(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS conversations (
            conversation_id TEXT PRIMARY KEY,
            last_message_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_conversations_recent ON conversations(last_message_at DESC);

        -- backfill inbox from existing messages (idempotent)
        INSERT OR IGNORE INTO conversations (conversation_id, last_message_at)
        SELECT conversation_id, MAX(created_at) FROM messages
        WHERE conversation_id LIKE 'conv:%' GROUP BY conversation_id;

        ALTER TABLE posts ADD COLUMN rsvp_event_id TEXT;

        CREATE INDEX IF NOT EXISTS idx_posts_rsvp_event ON posts(rsvp_event_id, created_at DESC);

        -- backfill RSVP e-tags (first 'e' tag value per kind-31924 row)
        UPDATE posts SET rsvp_event_id = (
            SELECT json_extract(je.value, '$[1]') FROM json_each(posts.tags_json) je
            WHERE json_extract(je.value, '$[0]') = 'e' LIMIT 1
        ) WHERE kind = 31924 AND rsvp_event_id IS NULL;

        ALTER TABLE reminders ADD COLUMN trigger_at INTEGER;

        CREATE INDEX IF NOT EXISTS idx_reminders_trigger ON reminders(trigger_at);

        UPDATE reminders SET trigger_at = start_time - minutes_before * 60
        WHERE trigger_at IS NULL;

        CREATE INDEX IF NOT EXISTS idx_posts_root_created ON posts(root_id, is_deleted, created_at ASC);

        CREATE INDEX IF NOT EXISTS idx_notifications_unread_type ON notifications(pubkey, is_read, type, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_posts_kind_deleted_created ON posts(kind, is_deleted, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_geohash_peers_purpose_seen ON geohash_peers(purpose, last_seen DESC);

        INSERT OR IGNORE INTO _migrations (version) VALUES (11);
    ",
    ))?;
    Ok(())
}
