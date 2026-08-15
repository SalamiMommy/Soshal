//! Migration 7: rebuild the posts_fts sync triggers with `INSERT OR REPLACE`
//! so a rowid collision with any pre-existing FTS row REPLACES it instead of
//! failing. v6 purged the known orphans; this makes a `constraint failed`
//! from posts_fts structurally impossible even if an orphan survives (old
//! device data, copied DBs, or a future writer).

use libsql::Connection;

pub fn v7_rebuild_fts_triggers(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        DROP TRIGGER IF EXISTS posts_ai;
        CREATE TRIGGER posts_ai AFTER INSERT ON posts BEGIN
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject)
            VALUES (new.rowid, new.id, new.pubkey, new.content, new.subject);
        END;

        DROP TRIGGER IF EXISTS posts_ad;
        CREATE TRIGGER posts_ad AFTER DELETE ON posts BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
        END;

        DROP TRIGGER IF EXISTS posts_au;
        CREATE TRIGGER posts_au AFTER UPDATE ON posts BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject)
            SELECT new.rowid, new.id, new.pubkey, new.content, new.subject WHERE new.is_deleted = 0;
        END;

        INSERT OR IGNORE INTO _migrations (version) VALUES (7);
    ",
    ))?;
    Ok(())
}
