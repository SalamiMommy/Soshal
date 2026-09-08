//! Migration 1: full canonical schema (tables, indexes, FTS5 virtual table,
//! and rowid-mapped sync triggers). Squashed from v001-v013 since app never shipped.

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
            relay_list TEXT DEFAULT '[]',
            follower_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS posts (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
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
            is_freenet_native INTEGER NOT NULL DEFAULT 0,
            rsvp_event_id TEXT,
            category TEXT,
            reposts_count INTEGER NOT NULL DEFAULT 0,
            event_lat REAL,
            event_lng REAL
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
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
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
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
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
            type TEXT NOT NULL,
            event_id TEXT,
            from_pubkey TEXT,
            content TEXT,
            created_at INTEGER NOT NULL,
            is_read INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS bookmarks (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
            event_id TEXT NOT NULL,
            title TEXT,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS blocks (
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
            blocked_pubkey TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (pubkey, blocked_pubkey)
        );

        CREATE TABLE IF NOT EXISTS ignored_notifications (
            pubkey TEXT NOT NULL,
            from_pubkey TEXT NOT NULL DEFAULT '',
            event_id TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (pubkey, kind, from_pubkey, event_id)
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
            group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
            role TEXT NOT NULL DEFAULT 'member',
            joined_at INTEGER NOT NULL,
            PRIMARY KEY (group_id, pubkey)
        );

        CREATE TABLE IF NOT EXISTS group_messages (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL DEFAULT '',
            sender_pubkey TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            sync_status INTEGER NOT NULL DEFAULT 0,
            is_deleted INTEGER NOT NULL DEFAULT 0
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
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
            url TEXT NOT NULL,
            file_hash TEXT,
            file_size INTEGER,
            mime_type TEXT,
            created_at INTEGER NOT NULL,
            blob_hash TEXT
        );

        CREATE TABLE IF NOT EXISTS group_roles (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
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
            trigger_at INTEGER,
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
            pubkey TEXT NOT NULL REFERENCES users(pubkey) ON DELETE CASCADE,
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
            buyer_confirmed INTEGER NOT NULL DEFAULT 0,
            seller_confirmed INTEGER NOT NULL DEFAULT 0,
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

        CREATE TABLE IF NOT EXISTS polls (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL DEFAULT '',
            question TEXT NOT NULL DEFAULT '',
            options TEXT NOT NULL DEFAULT '',
            expires_at INTEGER NOT NULL DEFAULT 0,
            closed INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS poll_votes (
            id TEXT PRIMARY KEY,
            poll_id TEXT NOT NULL DEFAULT '',
            option_id INTEGER NOT NULL DEFAULT 0,
            voter_pubkey TEXT NOT NULL DEFAULT '',
            voted_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS marketplace_reviews (
            id TEXT PRIMARY KEY,
            listing_id TEXT NOT NULL DEFAULT '',
            reviewer_pubkey TEXT NOT NULL DEFAULT '',
            rating INTEGER NOT NULL DEFAULT 0,
            text TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS spam_reports (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL DEFAULT '',
            target_id TEXT,
            target_pubkey TEXT,
            reason TEXT,
            tags TEXT NOT NULL DEFAULT '[]',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS tx_nodes (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL DEFAULT '',
            payload_json TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'pending',
            created_at INTEGER NOT NULL DEFAULT 0
        );

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

        CREATE TABLE IF NOT EXISTS diagnostic_logs (
            id TEXT PRIMARY KEY,
            level TEXT NOT NULL DEFAULT '',
            service TEXT NOT NULL DEFAULT '',
            method TEXT NOT NULL DEFAULT '',
            message TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS do_not_refetch_items (
            id TEXT PRIMARY KEY,
            pubkey TEXT,
            reason TEXT,
            created_at INTEGER NOT NULL DEFAULT 0
        );

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

        CREATE TABLE IF NOT EXISTS geohash_peers (
            pubkey TEXT PRIMARY KEY,
            geohash TEXT NOT NULL DEFAULT '',
            purpose TEXT NOT NULL DEFAULT '',
            first_seen INTEGER NOT NULL DEFAULT 0,
            last_seen INTEGER NOT NULL DEFAULT 0
        );

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

        CREATE TABLE IF NOT EXISTS stream_chat (
            id TEXT PRIMARY KEY,
            stream_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            text TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

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

        CREATE TABLE IF NOT EXISTS huddle_posts (
            id TEXT PRIMARY KEY,
            huddle_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            expires_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS banned_members (
            group_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            banned_by TEXT NOT NULL DEFAULT '',
            reason TEXT NOT NULL DEFAULT '',
            banned_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (group_id, pubkey)
        );

        CREATE TABLE IF NOT EXISTS group_join_requests (
            group_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT '',
            requested_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (group_id, pubkey)
        );

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

        CREATE TABLE IF NOT EXISTS musicloud_comments (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            content TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS story_reactions (
            story_id TEXT NOT NULL DEFAULT '',
            pubkey TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (story_id, pubkey, emoji)
        );

        CREATE TABLE IF NOT EXISTS muted_conversations (
            conversation_id TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS dating_unmatches (
            pubkey TEXT PRIMARY KEY,
            unmatched_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS ignored_entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL, -- 'notification', 'user', 'thread', 'category'
            target_id TEXT NOT NULL,
            owner_pubkey TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS secret_crushes (
            owner_pubkey TEXT NOT NULL,
            crush_pubkey TEXT NOT NULL,
            blinded_hash TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (owner_pubkey, crush_pubkey)
        );

        CREATE TABLE IF NOT EXISTS marketplace_offers (
            id TEXT PRIMARY KEY,
            listing_id TEXT NOT NULL,
            buyer_pubkey TEXT NOT NULL,
            seller_pubkey TEXT NOT NULL,
            amount_sats INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'accepted', 'declined', 'countered'
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS marketplace_saved (
            listing_id TEXT NOT NULL,
            pubkey TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (listing_id, pubkey)
        );

        CREATE TABLE IF NOT EXISTS musicloud_timed_comments (
            id TEXT PRIMARY KEY,
            track_id TEXT NOT NULL,
            pubkey TEXT NOT NULL,
            timestamp_ms INTEGER NOT NULL DEFAULT 0,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS musicloud_playlists (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL,
            title TEXT NOT NULL,
            is_private INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS group_shared_keys (
            group_id TEXT PRIMARY KEY,
            key_hex TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS conversations (
            conversation_id TEXT PRIMARY KEY,
            last_message_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(
            id UNINDEXED,
            pubkey UNINDEXED,
            content,
            subject,
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS users_fts USING fts5(
            pubkey UNINDEXED,
            npub,
            name,
            display_name,
            tokenize='unicode61 remove_diacritics 2'
        );

        -- Feed lookups
        CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_reply_to ON posts(reply_to);
        CREATE INDEX IF NOT EXISTS idx_posts_root_id ON posts(root_id);
        CREATE INDEX IF NOT EXISTS idx_posts_feed_cursor ON posts(created_at DESC, id DESC) WHERE is_deleted = 0 AND kind = 1;
        CREATE INDEX IF NOT EXISTS idx_posts_pubkey_created ON posts(pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_freenet_key ON posts(freenet_key);
        CREATE INDEX IF NOT EXISTS idx_posts_scheduled ON posts(pubkey, scheduled_at ASC);
        CREATE INDEX IF NOT EXISTS idx_posts_pubkey_scheduled_deleted ON posts(pubkey, is_deleted, scheduled_at ASC);
        CREATE INDEX IF NOT EXISTS idx_posts_rsvp_event ON posts(rsvp_event_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_kind_category ON posts(kind, category, is_deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_kind_rsvp ON posts(kind, rsvp_event_id);
        CREATE INDEX IF NOT EXISTS idx_posts_root_created ON posts(root_id, is_deleted, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_posts_kind_deleted_created ON posts(kind, is_deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_event_lat_lng ON posts(event_lat, event_lng);
        CREATE INDEX IF NOT EXISTS idx_posts_kind_pubkey_deleted_created ON posts(kind, pubkey, is_deleted, created_at DESC, id);
        CREATE INDEX IF NOT EXISTS idx_posts_kind_rsvp_pubkey_created ON posts(kind, rsvp_event_id, pubkey, created_at DESC, id);

        -- Messages
        CREATE INDEX IF NOT EXISTS idx_messages_pubkey ON messages(pubkey);
        CREATE INDEX IF NOT EXISTS idx_messages_conversation_desc ON messages(conversation_id, is_deleted, created_at DESC, id DESC);

        -- Reactions
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
        CREATE INDEX IF NOT EXISTS idx_notifications_unread_created ON notifications(pubkey, is_read, created_at DESC);

        -- Bookmarks
        CREATE INDEX IF NOT EXISTS idx_bookmarks_pubkey_created ON bookmarks(pubkey, created_at DESC);

        -- Media blobs
        CREATE INDEX IF NOT EXISTS idx_media_hash ON media_blobs(file_hash);
        CREATE INDEX IF NOT EXISTS idx_media_pubkey_created ON media_blobs(pubkey, created_at DESC);

        -- Group roles
        CREATE INDEX IF NOT EXISTS idx_group_roles_group ON group_roles(group_id);
        CREATE INDEX IF NOT EXISTS idx_group_roles_group_position ON group_roles(group_id, position ASC);

        -- Audit logs
        CREATE INDEX IF NOT EXISTS idx_audit_logs_group ON audit_logs(group_id);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_created ON audit_logs(actor_pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs(created_at DESC);

        -- Reminders
        CREATE INDEX IF NOT EXISTS idx_reminders_event ON reminders(event_id);
        CREATE INDEX IF NOT EXISTS idx_reminders_start ON reminders(start_time);
        CREATE INDEX IF NOT EXISTS idx_reminders_trigger ON reminders(trigger_at);

        -- Reposts / hashtags
        CREATE INDEX IF NOT EXISTS idx_reposts_event ON reposts(event_id);
        CREATE INDEX IF NOT EXISTS idx_hashtags_count ON hashtags(count DESC);
        CREATE INDEX IF NOT EXISTS idx_hashtags_tag_count ON hashtags(tag, count DESC);

        -- Group members
        CREATE INDEX IF NOT EXISTS idx_group_members_pubkey ON group_members(pubkey);
        CREATE INDEX IF NOT EXISTS idx_group_members_group_joined ON group_members(group_id, joined_at ASC);

        -- Ephemeral media
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_message ON ephemeral_media(message_id);
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_recipient_created ON ephemeral_media(recipient_pubkey, state, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ephemeral_media_expires ON ephemeral_media(expires_at) WHERE expires_at IS NOT NULL;

        -- Escrows
        CREATE INDEX IF NOT EXISTS idx_escrows_listing_created ON escrows(listing_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_escrows_buyer_created ON escrows(buyer_pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_escrows_seller_created ON escrows(seller_pubkey, created_at DESC);

        -- Groups
        CREATE INDEX IF NOT EXISTS idx_groups_updated_at ON groups(updated_at DESC);

        -- Users
        CREATE INDEX IF NOT EXISTS idx_users_name ON users(name);
        CREATE INDEX IF NOT EXISTS idx_users_display_name ON users(display_name);
        CREATE INDEX IF NOT EXISTS idx_users_npub ON users(npub);
        CREATE INDEX IF NOT EXISTS idx_users_follower_count ON users(follower_count DESC);

        -- Post views
        CREATE INDEX IF NOT EXISTS idx_post_views_seen ON post_views(pubkey, seen_at DESC);
        CREATE INDEX IF NOT EXISTS idx_post_views_post_id ON post_views(post_id);

        -- Additional indexes from migrations
        CREATE INDEX IF NOT EXISTS idx_group_messages_group_id ON group_messages(group_id);
        CREATE INDEX IF NOT EXISTS idx_group_messages_fetch ON group_messages(group_id, is_deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_polls_pubkey ON polls(pubkey);
        CREATE INDEX IF NOT EXISTS idx_poll_votes_poll_id ON poll_votes(poll_id);
        CREATE INDEX IF NOT EXISTS idx_marketplace_reviews_listing_id ON marketplace_reviews(listing_id);
        CREATE INDEX IF NOT EXISTS idx_spam_reports_target_pubkey ON spam_reports(target_pubkey);
        CREATE INDEX IF NOT EXISTS idx_tx_nodes_created_at ON tx_nodes(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_outbox_queue_status ON outbox_queue(status);
        CREATE INDEX IF NOT EXISTS idx_outbox_queue_pending ON outbox_queue(status, next_retry_at);
        CREATE INDEX IF NOT EXISTS idx_outbox_queue_created ON outbox_queue(status, created_at);
        CREATE INDEX IF NOT EXISTS idx_diagnostic_logs_level_created ON diagnostic_logs(level, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_diagnostic_logs_created ON diagnostic_logs(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_do_not_refetch_items_created ON do_not_refetch_items(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_custom_profile_nodes_user ON custom_profile_nodes(user_pubkey);
        CREATE INDEX IF NOT EXISTS idx_geohash_peers_geohash ON geohash_peers(geohash, last_seen DESC);
        CREATE INDEX IF NOT EXISTS idx_geohash_peers_purpose ON geohash_peers(purpose);
        CREATE INDEX IF NOT EXISTS idx_geohash_peers_purpose_seen ON geohash_peers(purpose, last_seen DESC);
        CREATE INDEX IF NOT EXISTS idx_geohash_peers_last_seen ON geohash_peers(last_seen);
        CREATE INDEX IF NOT EXISTS idx_link_previews_cached ON link_previews(cached_at DESC);
        CREATE INDEX IF NOT EXISTS idx_stream_chat_stream ON stream_chat(stream_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_guestbook_entries_profile ON guestbook_entries(profile_pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_huddle_posts_huddle ON huddle_posts(huddle_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_huddle_posts_expires ON huddle_posts(expires_at);
        CREATE INDEX IF NOT EXISTS idx_banned_members_group ON banned_members(group_id, banned_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_join_requests_group ON group_join_requests(group_id, status);
        CREATE INDEX IF NOT EXISTS idx_group_invites_group ON group_invites(group_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_group_invites_token ON group_invites(token);
        CREATE INDEX IF NOT EXISTS idx_musiclouds_created ON musiclouds(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_musiclouds_pubkey ON musiclouds(pubkey, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_musicloud_comments_track ON musicloud_comments(track_id, created_at ASC);
        CREATE INDEX IF NOT EXISTS idx_story_reactions_story ON story_reactions(story_id, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_conversations_recent ON conversations(last_message_at DESC);

        -- FTS5 sync triggers, rowid-mapped: post updates/deletes hit a single
        -- FTS row instead of scanning on id.
        CREATE TRIGGER IF NOT EXISTS posts_ai AFTER INSERT ON posts WHEN new.is_deleted = 0 BEGIN
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject)
            VALUES (new.rowid, new.id, new.pubkey, new.content, new.subject);
        END;

        CREATE TRIGGER IF NOT EXISTS posts_ad AFTER DELETE ON posts BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
        END;

        CREATE TRIGGER IF NOT EXISTS posts_au AFTER UPDATE ON posts BEGIN
            DELETE FROM posts_fts WHERE rowid = old.rowid;
            INSERT OR REPLACE INTO posts_fts(rowid, id, pubkey, content, subject)
            SELECT new.rowid, new.id, new.pubkey, new.content, new.subject WHERE new.is_deleted = 0;
        END;

        -- Users FTS triggers
        CREATE TRIGGER IF NOT EXISTS users_ai AFTER INSERT ON users BEGIN
            INSERT INTO users_fts(rowid, pubkey, npub, name, display_name)
            VALUES (new.rowid, new.pubkey, new.npub, coalesce(new.name,''), coalesce(new.display_name,''));
        END;

        CREATE TRIGGER IF NOT EXISTS users_ad AFTER DELETE ON users BEGIN
            DELETE FROM users_fts WHERE rowid = old.rowid;
        END;

        CREATE TRIGGER IF NOT EXISTS users_au AFTER UPDATE OF name, display_name, npub ON users
        WHEN old.name IS NOT new.name OR old.display_name IS NOT new.display_name OR old.npub != new.npub
        BEGIN
            DELETE FROM users_fts WHERE rowid = old.rowid;
            INSERT OR REPLACE INTO users_fts(rowid, pubkey, npub, name, display_name)
            SELECT new.rowid, new.pubkey, new.npub, coalesce(new.name,''), coalesce(new.display_name,'');
        END;

        -- Reposts count triggers
        CREATE TRIGGER IF NOT EXISTS reposts_ai AFTER INSERT ON reposts BEGIN
            UPDATE posts SET reposts_count = reposts_count + 1 WHERE id = NEW.event_id;
        END;
        CREATE TRIGGER IF NOT EXISTS reposts_ad AFTER DELETE ON reposts BEGIN
            UPDATE posts SET reposts_count = MAX(reposts_count - 1, 0) WHERE id = OLD.event_id;
        END;

        INSERT OR IGNORE INTO _migrations (version) VALUES (1);
    ",
    ))?;
    Ok(())
}
