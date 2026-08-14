//! Migration 3: polls, poll_votes, marketplace_reviews and spam_reports
//! tables + lookup indexes. Tables were referenced by the FFI marketplace /
//! moderation repos but absent from the v1 schema; columns match the repo
//! INSERT statements exactly.

use libsql::Connection;

pub fn v3_create_social_tables(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS polls (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL DEFAULT '',
            question TEXT NOT NULL DEFAULT '',
            options TEXT NOT NULL DEFAULT '',
            expires_at INTEGER NOT NULL DEFAULT 0,
            closed INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Poll lookups
        CREATE INDEX IF NOT EXISTS idx_polls_pubkey ON polls(pubkey);

        CREATE TABLE IF NOT EXISTS poll_votes (
            id TEXT PRIMARY KEY,
            poll_id TEXT NOT NULL DEFAULT '',
            option_id INTEGER NOT NULL DEFAULT 0,
            voter_pubkey TEXT NOT NULL DEFAULT '',
            voted_at INTEGER NOT NULL DEFAULT 0
        );

        -- Poll vote lookups
        CREATE INDEX IF NOT EXISTS idx_poll_votes_poll_id ON poll_votes(poll_id);

        CREATE TABLE IF NOT EXISTS marketplace_reviews (
            id TEXT PRIMARY KEY,
            listing_id TEXT NOT NULL DEFAULT '',
            reviewer_pubkey TEXT NOT NULL DEFAULT '',
            rating INTEGER NOT NULL DEFAULT 0,
            text TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Marketplace review lookups
        CREATE INDEX IF NOT EXISTS idx_marketplace_reviews_listing_id ON marketplace_reviews(listing_id);

        CREATE TABLE IF NOT EXISTS spam_reports (
            id TEXT PRIMARY KEY,
            pubkey TEXT NOT NULL DEFAULT '',
            target_id TEXT,
            target_pubkey TEXT,
            reason TEXT,
            tags TEXT NOT NULL DEFAULT '[]',
            created_at INTEGER NOT NULL DEFAULT 0
        );

        -- Spam report lookups
        CREATE INDEX IF NOT EXISTS idx_spam_reports_target_pubkey ON spam_reports(target_pubkey);

        INSERT OR IGNORE INTO _migrations (version) VALUES (3);
    ",
    ))?;
    Ok(())
}
