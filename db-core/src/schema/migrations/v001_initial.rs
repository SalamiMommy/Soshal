//! Migration 1: full canonical schema (tables, indexes, FTS5 virtual table,
//! and rowid-mapped sync triggers). App never shipped, so no legacy-compat
//! paths needed.

use libsql::Connection;

pub fn v1_create_tables(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS users (
            pubkey TEXT PRIMARY KEY,
            npub TEXT NOT NULL,
            name TEXT,
            display_name TEXT,
            about TEXT,
            picture TEXT,
            banner TEXT,
            nip05 TEXT,
            lud16 TEXT,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            metadata_json TEXT,
            contact_pubkeys TEXT DEFAULT '[]',
            relay_list TEXT DEFAULT '[]'
        );

        CREATE TABLE IF NOT EXISTS posts (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            content TEXT NOT NULL DEFAULT '',
            kind INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            tags_json TEXT NOT NULL DEFAULT '[]',
            sig TEXT,
            reply_to TEXT,
            root_id TEXT,
            mentioned_pubkeys TEXT DEFAULT '[]',
            mentioned_hashtags TEXT DEFAULT '[]',
            subject TEXT,
            sync_status TEXT NOT NULL DEFAULT 'pending',
            is_deleted INTEGER NOT NULL DEFAULT 0,
            scheduled_at INTEGER,
            freenet_key TEXT,
            is_freenet_native INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            tags_json TEXT DEFAULT '[]',
            reply_to TEXT,
            sync_status TEXT NOT NULL DEFAULT 'pending',
            is_deleted INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS reactions (
            id TEXT PRIMARY KEY,
            event_id TEXT NOT NULL,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            content TEXT NOT NULL DEFAULT '+',
            created_at INTEGER NOT NULL,
            kind INTEGER NOT NULL DEFAULT 7
        );

        CREATE TABLE IF NOT EXISTS zaps (
            id TEXT PRIMARY KEY,
            event_id TEXT,
            recipient_pubkey TEXT NOT NULL,
            sender_pubkey TEXT,
            amount_msat INTEGER NOT NULL,
            bolt11 TEXT,
            preimage TEXT,
            comment TEXT,
            created_at INTEGER NOT NULL,
            pubkey TEXT,
            amount INTEGER NOT NULL DEFAULT 0,
            content TEXT,
            zap_type TEXT NOT NULL DEFAULT 'public'
        );

        CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            type TEXT NOT NULL,
            event_id TEXT,
            from_pubkey TEXT,
            content TEXT,
            created_at INTEGER NOT NULL,
            is_read INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS bookmarks (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            event_id TEXT NOT NULL,
            title TEXT,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS blocks (
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            blocked_pubkey TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (pubkey, blocked_pubkey)
        );

        CREATE TABLE IF NOT EXISTS groups (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            about TEXT,
            picture TEXT,
            pubkey TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            access_type TEXT NOT NULL DEFAULT 'open',
            relay TEXT,
            sync_status TEXT NOT NULL DEFAULT 'synced'
        );

        CREATE TABLE IF NOT EXISTS group_members (
            group_id TEXT NOT NULL REFERENCES groups(id),
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            role TEXT NOT NULL DEFAULT 'member',
            joined_at INTEGER NOT NULL,
            PRIMARY KEY (group_id, pubkey)
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS relays (
            url TEXT PRIMARY KEY,
            pubkey TEXT,
            name TEXT,
            read_enabled INTEGER NOT NULL DEFAULT 1,
            write_enabled INTEGER NOT NULL DEFAULT 1,
            priority INTEGER NOT NULL DEFAULT 0,
            last_connected_at INTEGER,
            health_score REAL NOT NULL DEFAULT 1.0
        );

        CREATE TABLE IF NOT EXISTS media_blobs (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            url TEXT NOT NULL,
            file_hash TEXT,
            file_size INTEGER,
            mime_type TEXT,
            created_at INTEGER NOT NULL,
            blob_hash TEXT
        );

        CREATE TABLE IF NOT EXISTS group_roles (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL REFERENCES groups(id),
            name TEXT NOT NULL,
            permissions TEXT NOT NULL DEFAULT '[]',
            created_at INTEGER NOT NULL,
            color TEXT NOT NULL DEFAULT '#000000',
            position INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS audit_logs (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL,
            actor_pubkey TEXT NOT NULL,
            action TEXT NOT NULL,
            target_pubkey TEXT,
            details TEXT,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS reminders (
            id TEXT PRIMARY KEY,
            event_id TEXT NOT NULL,
            title TEXT NOT NULL,
            start_time INTEGER NOT NULL,
            minutes_before INTEGER NOT NULL DEFAULT 10,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS hashtags (
            tag TEXT NOT NULL,
            pubkey TEXT NOT NULL,
            last_used_at INTEGER NOT NULL,
            count INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (tag, pubkey)
        );

        CREATE TABLE IF NOT EXISTS reposts (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey),
            event_id TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS post_views (
            pubkey TEXT NOT NULL,
            post_id TEXT NOT NULL,
            seen_at INTEGER NOT NULL,
            PRIMARY KEY (pubkey, post_id)
        );

        CREATE TABLE IF NOT EXISTS custom_profiles (
            pubkey TEXT PRIMARY KEY,
            data TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS escrows (
            id TEXT PRIMARY KEY,
            listing_id TEXT NOT NULL,
            buyer_pubkey TEXT NOT NULL,
            seller_pubkey TEXT NOT NULL,
            amount_msats INTEGER NOT NULL,
            currency TEXT NOT NULL DEFAULT 'sats',
            status TEXT NOT NULL DEFAULT 'created',
            escrow_note TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ephemeral_media (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            conversation_type TEXT NOT NULL DEFAULT 'dm',
            media_url TEXT NOT NULL,
            media_type TEXT NOT NULL DEFAULT 'image',
            sender_pubkey TEXT NOT NULL,
            recipient_pubkey TEXT NOT NULL,
            max_views INTEGER NOT NULL DEFAULT 1,
            current_views INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL DEFAULT 'pending',
            expires_at INTEGER,
            created_at INTEGER NOT NULL,
            viewed_at INTEGER
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
            id UNINDEXED,
            pubkey UNINDEXED,
            content,
            subject,
            tokenize='unicode61 remove_diacritics 2'
        );

        -- Feed lookups
        CREATE INDEX IF NOT EXISTS idx_posts_pubkey ON posts(pubkey);
        CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_reply_to ON posts(reply_to);
        CREATE INDEX IF NOT EXISTS idx_posts_root_id ON posts(root_id);
        CREATE INDEX IF NOT EXISTS idx_posts_kind ON posts(kind);
        CREATE INDEX IF NOT EXISTS idx_posts_pubkey_created ON posts(pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_kind_created ON posts(kind, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_feed_lookup ON posts(pubkey, is_deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_recent_lookup ON posts(is_deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_freenet_key ON posts(freenet_key);
        CREATE INDEX IF NOT EXISTS idx_posts_scheduled ON posts(pubkey, scheduled_at ASC);
        CREATE INDEX IF NOT EXISTS idx_posts_pubkey_scheduled_deleted ON posts(pubkey, is_deleted, scheduled_at ASC);

        -- Messages
        CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_messages_pubkey ON messages(pubkey);

        -- Reactions
        CREATE INDEX IF NOT EXISTS idx_reactions_event ON reactions(event_id);
        CREATE INDEX IF NOT EXISTS idx_reactions_pubkey ON reactions(pubkey);
        CREATE INDEX IF NOT EXISTS idx_reactions_event_created ON reactions(event_id, created_at DESC);

        -- Zaps
        CREATE INDEX IF NOT EXISTS idx_zaps_event ON zaps(event_id);
        CREATE INDEX IF NOT EXISTS idx_zaps_recipient ON zaps(recipient_pubkey);
        CREATE INDEX IF NOT EXISTS idx_zaps_sender ON zaps(sender_pubkey);
        CREATE INDEX IF NOT EXISTS idx_zaps_pubkey_created ON zaps(pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_zaps_event_amount ON zaps(event_id, amount);
        CREATE INDEX IF NOT EXISTS idx_zaps_recipient_created ON zaps(recipient_pubkey, created_at DESC);

        -- Notifications
        CREATE INDEX IF NOT EXISTS idx_notifications_pubkey ON notifications(pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(pubkey, is_read);
        CREATE INDEX IF NOT EXISTS idx_notifications_unread_created ON notifications(pubkey, is_read, created_at DESC);

        -- Bookmarks
        CREATE INDEX IF NOT EXISTS idx_bookmarks_pubkey ON bookmarks(pubkey);
        CREATE INDEX IF NOT EXISTS idx_bookmarks_pubkey_created ON bookmarks(pubkey, created_at DESC);

        -- Media blobs
        CREATE INDEX IF NOT EXISTS idx_media_pubkey ON media_blobs(pubkey);
        CREATE INDEX IF NOT EXISTS idx_media_hash ON media_blobs(file_hash);
        CREATE INDEX IF NOT EXISTS idx_media_pubkey_created ON media_blobs(pubkey, created_at DESC);

        -- Group roles
        CREATE INDEX IF NOT EXISTS idx_group_roles_group ON group_roles(group_id);
        CREATE INDEX IF NOT EXISTS idx_group_roles_group_position ON group_roles(group_id, position ASC);

        -- Audit logs
        CREATE INDEX IF NOT EXISTS idx_audit_logs_group ON audit_logs(group_id);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_actor ON audit_logs(actor_pubkey);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_created ON audit_logs(actor_pubkey, created_at DESC);

        -- Reminders
        CREATE INDEX IF NOT EXISTS idx_reminders_event ON reminders(event_id);

        -- Reposts / hashtags
        CREATE INDEX IF NOT EXISTS idx_reposts_event ON reposts(event_id);
        CREATE INDEX IF NOT EXISTS idx_hashtags_count ON hashtags(count DESC);

        -- Group members
        CREATE INDEX IF NOT EXISTS idx_group_members_pubkey ON group_members(pubkey);
        CREATE INDEX IF NOT EXISTS idx_group_members_group_joined ON group_members(group_id, joined_at ASC);

        -- Ephemeral media
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_recipient ON ephemeral_media(recipient_pubkey, state);
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_message ON ephemeral_media(message_id);
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_recipient_created ON ephemeral_media(recipient_pubkey, state, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_expires ON ephemeral_media(expires_at) WHERE expires_at IS NOT NULL;

        -- Escrows
        CREATE INDEX IF NOT EXISTS idx_escrows_listing ON escrows(listing_id);
        CREATE INDEX IF NOT EXISTS idx_escrows_buyer ON escrows(buyer_pubkey);
        CREATE INDEX IF NOT EXISTS idx_escrows_seller ON escrows(seller_pubkey);
        CREATE INDEX IF NOT EXISTS idx_escrows_listing_created ON escrows(listing_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_escrows_buyer_created ON escrows(buyer_pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_escrows_seller_created ON escrows(seller_pubkey, created_at DESC);

        -- Groups
        CREATE INDEX IF NOT EXISTS idx_groups_updated_at ON groups(updated_at DESC);

        -- Users
        CREATE INDEX IF NOT EXISTS idx_users_name ON users(name);
        CREATE INDEX IF NOT EXISTS idx_users_display_name ON users(display_name);
        CREATE INDEX IF NOT EXISTS idx_users_npub ON users(npub);

        -- Post views
        CREATE INDEX IF NOT EXISTS idx_post_views_seen ON post_views(pubkey, seen_at DESC);

        -- FTS5 sync triggers, rowid-mapped: post updates/deletes hit a single
        -- FTS row instead of scanning on id.
        CREATE TRIGGER IF NOT EXISTS posts_ai AFTER INSERT ON posts BEGIN
            INSERT INTO posts_fts(rowid, id, pubkey, content, subject)
            VALUES (new.rowid, new.id, new.pubkey, new.content, new.subject);
        END;

        CREATE TRIGGER IF NOT EXISTS posts_ad AFTER DELETE ON posts BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
        END;

        CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE ON posts BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
            INSERT INTO posts_fts(rowid, id, pubkey, content, subject)
            SELECT new.rowid, new.id, new.pubkey, new.content, new.subject WHERE new.is_deleted = 0;
        END;

        INSERT OR IGNORE INTO _migrations (version) VALUES (1);
    ",
    ))?;
    Ok(())
}
