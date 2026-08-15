//! Migration 6: purge orphan `posts_fts` rows created by the pre-fix
//! `search_index.rs` (auto-assigned rowids, never removable by delete-by-id).
//! Those orphans collided with the `posts_ai` trigger's explicit
//! `rowid = new.rowid` inserts and surfaced as a bare `constraint failed`
//! during sync ingest. Only positive rowids without a backing posts row are
//! removed: posts-backed trigger rows match posts.rowid, and post-fix profile
//! rows live in negative rowid space.

use libsql::Connection;

pub fn v6_purge_orphan_fts_rows(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        DELETE FROM posts_fts
        WHERE rowid > 0
          AND rowid NOT IN (SELECT rowid FROM posts);

        INSERT OR IGNORE INTO _migrations (version) VALUES (6);
    ",
    ))?;
    Ok(())
}
