//! Migration 12: saved content + playlist tracks.
//! `saved_content` backs per-user Saved tabs for Minis (kind 31020) and
//! Musicloud (kind 31022); `musicloud_playlist_tracks` maps tracks into user
//! playlists (`musicloud_playlists` from v001).

use libsql::Connection;

pub fn v12_saved_content_playlists(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
CREATE TABLE IF NOT EXISTS saved_content (
    kind INTEGER NOT NULL,
    id TEXT NOT NULL,
    pubkey TEXT NOT NULL DEFAULT '',
    d TEXT NOT NULL DEFAULT '',
    media_type TEXT NOT NULL DEFAULT '',
    media_url TEXT NOT NULL DEFAULT '',
    text_overlay TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    thumbnail TEXT NOT NULL DEFAULT '',
    blob_hash TEXT NOT NULL DEFAULT '',
    media_size INTEGER NOT NULL DEFAULT 0,
    audience TEXT NOT NULL DEFAULT 'public',
    hashtags TEXT NOT NULL DEFAULT '[]',
    host_ready INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    saved_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (kind, id)
);
CREATE INDEX IF NOT EXISTS idx_saved_content_saved_at ON saved_content (saved_at DESC);
CREATE INDEX IF NOT EXISTS idx_saved_content_kind_saved ON saved_content (kind, saved_at DESC);

CREATE TABLE IF NOT EXISTS musicloud_playlists (
    id TEXT PRIMARY KEY,
    pubkey TEXT NOT NULL,
    title TEXT NOT NULL,
    is_private INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS musicloud_playlist_tracks (
    playlist_id TEXT NOT NULL,
    track_id TEXT NOT NULL,
    pubkey TEXT NOT NULL DEFAULT '',
    d TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    thumbnail TEXT NOT NULL DEFAULT '',
    audio_url TEXT NOT NULL DEFAULT '',
    blob_hash TEXT NOT NULL DEFAULT '',
    media_size INTEGER NOT NULL DEFAULT 0,
    audience TEXT NOT NULL DEFAULT 'public',
    hashtags TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (playlist_id, track_id)
);
CREATE INDEX IF NOT EXISTS idx_playlist_tracks_pos ON musicloud_playlist_tracks (playlist_id, position);

INSERT OR IGNORE INTO _migrations (version) VALUES (12);
        ",
    ))?;
    Ok(())
}
