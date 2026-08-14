//! Migration 5: tables referenced by repos but missing from the v1 schema
//! (`custom_profile_nodes` vs `custom_profiles`, etc.). Columns match the repo
//! INSERT/UPDATE/SELECT statements exactly; nullable columns stay nullable
//! (repos bind NULL explicitly via Option fields).

use libsql::Connection;

pub fn v5_create_missing_tables(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS diagnostic_logs (
            id TEXT PRIMARY KEY,
            level TEXT NOT NULL DEFAULT '',
            service TEXT NOT NULL DEFAULT '',
            method TEXT NOT NULL DEFAULT '',
            message TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Diagnostic log lookups
        CREATE INDEX IF NOT EXISTS idx_diagnostic_logs_level_created ON diagnostic_logs(level, created_at DESC);

        CREATE TABLE IF NOT EXISTS do_not_refetch_items (
            id TEXT PRIMARY KEY,
            pubkey TEXT,
            reason TEXT,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Refetch list ordering
        CREATE INDEX IF NOT EXISTS idx_do_not_refetch_items_created ON do_not_refetch_items(created_at DESC);

        CREATE TABLE IF NOT EXISTS custom_profile_nodes (
            id TEXT PRIMARY KEY,
            user_pubkey TEXT NOT NULL DEFAULT '',
            type TEXT NOT NULL DEFAULT '',
            styles TEXT NOT NULL DEFAULT '',
            properties TEXT NOT NULL DEFAULT '',
            layout_row INTEGER NOT NULL DEFAULT 0,
            layout_col INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0
        );

        -- Profile node lookups
        CREATE INDEX IF NOT EXISTS idx_custom_profile_nodes_user ON custom_profile_nodes(user_pubkey);

        CREATE TABLE IF NOT EXISTS geohash_peers (
            pubkey TEXT PRIMARY KEY,
            geohash TEXT NOT NULL DEFAULT '',
            purpose TEXT NOT NULL DEFAULT '',
            first_seen INTEGER NOT NULL DEFAULT 0,
            last_seen INTEGER NOT NULL DEFAULT 0
        );

        -- Geohash peer lookups
        CREATE INDEX IF NOT EXISTS idx_geohash_peers_geohash ON geohash_peers(geohash, last_seen DESC);
        CREATE INDEX IF NOT EXISTS idx_geohash_peers_purpose ON geohash_peers(purpose);

        CREATE TABLE IF NOT EXISTS friend_backups (
            user_pubkey TEXT PRIMARY KEY,
            encrypted_data TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS link_previews (
            url TEXT PRIMARY KEY,
            domain TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            image TEXT,
            favicon TEXT,
            cached_at INTEGER NOT NULL DEFAULT 0
        );

        -- Link preview ordering
        CREATE INDEX IF NOT EXISTS idx_link_previews_cached ON link_previews(cached_at DESC);

        CREATE TABLE IF NOT EXISTS stream_chat (
            id TEXT PRIMARY KEY,
            stream_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            text TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Stream chat lookups
        CREATE INDEX IF NOT EXISTS idx_stream_chat_stream ON stream_chat(stream_id, created_at ASC);

        CREATE TABLE IF NOT EXISTS guestbook_entries (
            id TEXT PRIMARY KEY,
            profile_pubkey TEXT NOT NULL DEFAULT '',
            sender_pubkey TEXT NOT NULL DEFAULT '',
            sender_name TEXT,
            sender_avatar TEXT,
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            signature TEXT,
            approved INTEGER NOT NULL DEFAULT 0
        );

        -- Guestbook lookups
        CREATE INDEX IF NOT EXISTS idx_guestbook_entries_profile ON guestbook_entries(profile_pubkey, created_at DESC);

        CREATE TABLE IF NOT EXISTS huddle_posts (
            id TEXT PRIMARY KEY,
            huddle_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER NOT NULL DEFAULT 0
        );

        -- Huddle post lookups
        CREATE INDEX IF NOT EXISTS idx_huddle_posts_huddle ON huddle_posts(huddle_id, created_at ASC);

        CREATE TABLE IF NOT EXISTS banned_members (
            group_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            banned_by TEXT NOT NULL DEFAULT '',
            reason TEXT NOT NULL DEFAULT '',
            banned_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (group_id, pubkey)
        );

        -- Banned member lookups
        CREATE INDEX IF NOT EXISTS idx_banned_members_group ON banned_members(group_id, banned_at DESC);

        CREATE TABLE IF NOT EXISTS group_join_requests (
            group_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT '',
            requested_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (group_id, pubkey)
        );

        -- Join request lookups
        CREATE INDEX IF NOT EXISTS idx_group_join_requests_group ON group_join_requests(group_id, status);

        CREATE TABLE IF NOT EXISTS group_invites (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            created_by TEXT NOT NULL DEFAULT '',
            token TEXT NOT NULL DEFAULT '',
            max_uses INTEGER NOT NULL DEFAULT 0,
            uses INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Group invite lookups
        CREATE INDEX IF NOT EXISTS idx_group_invites_group ON group_invites(group_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_invites_token ON group_invites(token);

        CREATE TABLE IF NOT EXISTS musiclouds (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL DEFAULT '',
            audio_url TEXT NOT NULL DEFAULT '',
            title TEXT,
            duration INTEGER,
            text_overlay TEXT,
            thumbnail TEXT,
            likes INTEGER NOT NULL DEFAULT 0,
            liked INTEGER NOT NULL DEFAULT 0,
            bookmarked INTEGER NOT NULL DEFAULT 0,
            audience TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Musicloud lookups
        CREATE INDEX IF NOT EXISTS idx_musiclouds_created ON musiclouds(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_musiclouds_pubkey ON musiclouds(pubkey, created_at DESC);

        CREATE TABLE IF NOT EXISTS musicloud_comments (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Musicloud comment lookups
        CREATE INDEX IF NOT EXISTS idx_musicloud_comments_track ON musicloud_comments(track_id, created_at ASC);

        CREATE TABLE IF NOT EXISTS story_reactions (
            story_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (story_id, pubkey, emoji)
        );

        -- Story reaction lookups
        CREATE INDEX IF NOT EXISTS idx_story_reactions_story ON story_reactions(story_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS muted_conversations (
            conversation_id TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS dating_unmatches (
            pubkey TEXT PRIMARY KEY,
            unmatched_at INTEGER NOT NULL DEFAULT 0
        );

        INSERT OR IGNORE INTO _migrations (version) VALUES (5);
    ",
    ))?;
    Ok(())
}
