//! Migration 12: perf schema for round-5 sweep.
//!
//! - `posts.category` (kind-30402 first `t` tag denormalized at ingest) so
//!   marketplace category queries hit an index instead of a per-row
//!   `json_each` scan.
//! - `users.follower_count` (own contact-list length, same semantics as the
//!   old `ORDER BY length(contact_pubkeys)` scan) so trending profiles use an
//!   index instead of a correlated-subquery full scan.
//! - covering index for RSVP attendee counts (`kind, content, rsvp_event_id`).

use libsql::Connection;

pub fn v12_perf_schema(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        ALTER TABLE posts ADD COLUMN category TEXT;

        CREATE INDEX IF NOT EXISTS idx_posts_kind_category ON posts(kind, category, is_deleted);

        -- backfill listing categories (first 't' tag value per kind-30402 row)
        UPDATE posts SET category = (
            SELECT json_extract(je.value, '$[1]') FROM json_each(posts.tags_json) je
            WHERE json_extract(je.value, '$[0]') = 't' LIMIT 1
        ) WHERE kind = 30402 AND category IS NULL;

        ALTER TABLE users ADD COLUMN follower_count INTEGER NOT NULL DEFAULT 0;

        CREATE INDEX IF NOT EXISTS idx_users_follower_count ON users(follower_count DESC);

        -- backfill follower counts (own contact-list length, like the legacy
        -- trending scan measured)
        UPDATE users SET follower_count =
            CASE WHEN json_valid(contact_pubkeys)
                 THEN json_array_length(contact_pubkeys) ELSE 0 END
        WHERE contact_pubkeys IS NOT NULL AND contact_pubkeys != '';

        CREATE INDEX IF NOT EXISTS idx_posts_kind_content_rsvp ON posts(kind, content, rsvp_event_id);

        INSERT OR IGNORE INTO _migrations (version) VALUES (12);
    ",
    ))?;
    Ok(())
}
