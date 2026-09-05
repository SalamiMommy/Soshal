//! Migration 11: drop redundant indexes that are strict prefixes of
//! surviving covering indexes (pure write-amplification on hot tables).
//!
//! `idx_posts_kind_content_rsvp` additionally indexes the `content` BLOB —
//! pathological write-amplification — so it is replaced with a slim
//! `idx_posts_kind_rsvp(kind, rsvp_event_id)` to keep rsvp-by-kind lookups
//! served. Each surviving `_created` variant (or the v006 feed-cursor
//! covering index) still serves the prefix lookups.

use libsql::Connection;

pub fn v11_index_cleanup(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        -- Prefix of idx_posts_kind_deleted_created (kind, is_deleted, created_at)
        DROP INDEX IF EXISTS idx_posts_kind;

        -- BLOB column in an index = pathological write-amplification;
        -- replace with slim rsvp-by-kind index
        DROP INDEX IF EXISTS idx_posts_kind_content_rsvp;
        CREATE INDEX IF NOT EXISTS idx_posts_kind_rsvp ON posts(kind, rsvp_event_id);

        -- Prefix of idx_ephemeral_media_recipient_created (recipient_pubkey, state, created_at)
        DROP INDEX IF EXISTS idx_ephemeral_media_recipient;

        -- Prefixes of idx_escrows_buyer_created / idx_escrows_seller_created
        DROP INDEX IF EXISTS idx_escrows_buyer;
        DROP INDEX IF EXISTS idx_escrows_seller;

        -- Prefix of idx_group_members_group_joined (group_id, joined_at)
        DROP INDEX IF EXISTS idx_group_members_group;

        -- Strict prefix of v006 idx_posts_feed_cursor (pubkey, is_deleted, created_at, id)
        DROP INDEX IF EXISTS idx_posts_user_timeline;

        INSERT OR IGNORE INTO _migrations (version) VALUES (11);
        ",
    ))?;
    Ok(())
}
