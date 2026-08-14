//! Migration 4: sync-engine tables (tx_nodes, tx_edges, outbox_queue) +
//! lookup indexes. Tables were referenced by `sync-core` (tx.rs / outbox.rs)
//! but absent from the v1 schema; columns match the repo INSERT/UPDATE
//! statements exactly. `media_path` stays nullable: enqueue passes NULL when
//! no media is attached.

use libsql::Connection;

pub fn v4_create_sync_tables(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tx_nodes (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL DEFAULT '',
            payload_json TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Transaction order lookups
        CREATE INDEX IF NOT EXISTS idx_tx_nodes_created_at ON tx_nodes(created_at DESC);

        CREATE TABLE IF NOT EXISTS tx_edges (
            parent_id TEXT NOT NULL,
            child_id TEXT NOT NULL,
            PRIMARY KEY (parent_id, child_id)
        );

        CREATE TABLE IF NOT EXISTS outbox_queue (
            id TEXT PRIMARY KEY,
            action_type TEXT NOT NULL DEFAULT '',
            payload_json TEXT NOT NULL DEFAULT '',
            media_path TEXT DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending',
            retry_count INTEGER NOT NULL DEFAULT 0,
            next_retry_at INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Outbox status lookups
        CREATE INDEX IF NOT EXISTS idx_outbox_queue_status ON outbox_queue(status);

        INSERT OR IGNORE INTO _migrations (version) VALUES (4);
    ",
    ))?;
    Ok(())
}
