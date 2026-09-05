//! Migration 13: index posts `category` in FTS. The `posts_fts` virtual
//! table and its sync triggers (v001 + v009) never included the column, so
//! MATCH queries miss category tokens. Table + triggers are rebuilt with the
//! category column indexed in the same shape v001/v009 defined.

use libsql::Connection;

pub fn v13_category_fts(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        DROP TRIGGER IF EXISTS posts_ai;
        DROP TRIGGER IF EXISTS posts_au;
        DROP TABLE IF EXISTS posts_fts;

        CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
            id UNINDEXED,
            pubkey UNINDEXED,
            content,
            subject,
            category,
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE TRIGGER IF NOT EXISTS posts_ai AFTER INSERT ON posts WHEN new.is_deleted = 0 BEGIN
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject, category)
            VALUES (new.rowid, new.id, new.pubkey, new.content, new.subject, new.category);
        END;

        CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE OF content, subject, category, is_deleted ON posts
        WHEN old.content != new.content OR old.subject IS NOT new.subject OR old.category IS NOT new.category OR old.is_deleted != new.is_deleted
        BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject, category)
            SELECT new.rowid, new.id, new.pubkey, new.content, new.subject, new.category WHERE new.is_deleted = 0;
        END;

        INSERT INTO posts_fts(rowid, id, pubkey, content, subject, category)
        SELECT rowid, id, pubkey, content, subject, category FROM posts WHERE is_deleted = 0;

        INSERT INTO posts_fts(posts_fts) VALUES('rebuild');

        INSERT OR IGNORE INTO _migrations (version) VALUES (13);
        ",
    ))?;
    Ok(())
}
