//! Migration 2: group_messages table + group_id lookup index. Table was
//! referenced by the FFI groups module but absent from the v1 schema.

use libsql::Connection;

pub fn v2_create_group_messages(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS group_messages (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            sender_pubkey TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            sync_status INTEGER NOT NULL DEFAULT 0,
            is_deleted INTEGER NOT NULL DEFAULT 0
        );

        -- Group message lookups
        CREATE INDEX IF NOT EXISTS idx_group_messages_group_id ON group_messages(group_id);

        INSERT OR IGNORE INTO _migrations (version) VALUES (2);
    ",
    ))?;
    Ok(())
}
