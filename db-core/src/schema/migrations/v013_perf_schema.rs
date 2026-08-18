//! Migration 13: perf schema round 2.
//!
//! - `idx_post_views_post_id` so per-post view counts (dating/profile) hit an
//!   index.
//! - `idx_posts_kind_category` rebuilt carrying `created_at` so marketplace
//!   trending ORDER BY avoids a temp sort.
//! - `posts.reposts_count` denormalized (was correlated-subquery full scan),
//!   kept in sync by `reposts_ai`/`reposts_ad` triggers.
//! - `posts.event_lat`/`event_lng` denormalized from content JSON (was per-row
//!   json_extract scan in events_fetch_nearby), indexed.

use libsql::Connection;

pub fn v13_perf_schema(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_post_views_post_id ON post_views(post_id);

        DROP INDEX IF EXISTS idx_posts_kind_category;
        CREATE INDEX IF NOT EXISTS idx_posts_kind_category ON posts(kind, category, is_deleted, created_at DESC);

        ALTER TABLE posts ADD COLUMN reposts_count INTEGER NOT NULL DEFAULT 0;
        UPDATE posts SET reposts_count =
            (SELECT COUNT(*) FROM reposts rc WHERE rc.event_id = posts.id);

        CREATE TRIGGER IF NOT EXISTS reposts_ai AFTER INSERT ON reposts BEGIN
            UPDATE posts SET reposts_count = reposts_count + 1 WHERE id = NEW.event_id;
        END;
        CREATE TRIGGER IF NOT EXISTS reposts_ad AFTER DELETE ON reposts BEGIN
            UPDATE posts SET reposts_count = MAX(reposts_count - 1, 0) WHERE id = OLD.event_id;
        END;

        ALTER TABLE posts ADD COLUMN event_lat REAL;
        ALTER TABLE posts ADD COLUMN event_lng REAL;
        UPDATE posts SET event_lat = json_extract(content, '$.location.lat'),
                        event_lng = json_extract(content, '$.location.lng')
        WHERE json_valid(content) AND json_type(json_extract(content, '$.location')) = 'object';
        CREATE INDEX IF NOT EXISTS idx_posts_event_lat_lng ON posts(event_lat, event_lng);

        INSERT OR IGNORE INTO _migrations (version) VALUES (13);
    ",
    ))?;
    Ok(())
}