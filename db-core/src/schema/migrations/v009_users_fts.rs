//! Migration 9: FTS5 index over users (name/display_name/npub prefix search)
//! plus a covering index for hashtag trending. The old `LIKE '%term%'`
//! user search scanned the full table with unusable indexes; FTS5 gives
//! indexed prefix matching.

use libsql::Connection;

pub fn v9_users_fts(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS users_fts USING fts5(
            pubkey UNINDEXED,
            npub,
            name,
            display_name,
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE TRIGGER IF NOT EXISTS users_ai AFTER INSERT ON users BEGIN
            INSERT INTO users_fts(rowid, pubkey, npub, name, display_name)
            VALUES (new.rowid, new.pubkey, new.npub, coalesce(new.name,''), coalesce(new.display_name,''));
        END;

        CREATE TRIGGER IF NOT EXISTS users_ad AFTER DELETE ON users BEGIN
            DELETE FROM users_fts WHERE rowid = old.rowid;
        END;

        CREATE TRIGGER IF NOT EXISTS users_au AFTER UPDATE ON users BEGIN
            DELETE FROM users_fts WHERE rowid = old.rowid;
            INSERT OR REPLACE INTO users_fts(rowid, pubkey, npub, name, display_name)
            SELECT new.rowid, new.pubkey, new.npub, coalesce(new.name,''), coalesce(new.display_name,'');
        END;

        INSERT INTO users_fts(rowid, pubkey, npub, name, display_name)
        SELECT rowid, pubkey, npub, coalesce(name,''), coalesce(display_name,'') FROM users;

        CREATE INDEX IF NOT EXISTS idx_hashtags_tag_count ON hashtags(tag, count DESC);

        INSERT OR IGNORE INTO _migrations (version) VALUES (9);
    ",
    ))?;
    Ok(())
}
