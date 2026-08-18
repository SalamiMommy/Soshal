//! Migration 2: group rooms (themed chatrooms), threads (reddit-like),
//! voice channels with local presence, and room scoping for group messages.

use libsql::Connection;

pub fn v2_group_channels(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS group_rooms (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL DEFAULT '',
            topic TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            color TEXT NOT NULL DEFAULT '#8b5cf6',
            position INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        ALTER TABLE group_messages ADD COLUMN room_id TEXT NOT NULL DEFAULT '';

        CREATE TABLE IF NOT EXISTS group_threads (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            reply_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS group_thread_replies (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL DEFAULT '',
            parent_id TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS group_voice_channels (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL DEFAULT '',
            position INTEGER NOT NULL DEFAULT 0,
            created_by TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS group_voice_presence (
            channel_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            joined_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (channel_id, pubkey)
        );

        CREATE INDEX IF NOT EXISTS idx_group_rooms_group ON group_rooms(group_id, position ASC);
        CREATE INDEX IF NOT EXISTS idx_group_messages_room ON group_messages(group_id, room_id, is_deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_threads_group ON group_threads(group_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_threads_pinned ON group_threads(group_id, is_pinned DESC, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_thread_replies_thread ON group_thread_replies(thread_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_group_voice_channels_group ON group_voice_channels(group_id, position ASC);
        CREATE INDEX IF NOT EXISTS idx_group_voice_presence_pubkey ON group_voice_presence(pubkey);

        INSERT OR IGNORE INTO _migrations (version) VALUES (2);
        ",
    ))?;
    Ok(())
}
