use crate::repos::limits;
use crate::Database;
use libsql::params;

const POST_UPSERT_SQL: &str = "INSERT INTO posts (id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id, category) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18, CASE WHEN ?4 = 30402 THEN (SELECT json_extract(je.value, '$[1]') FROM json_each(CASE WHEN json_valid(?6) THEN ?6 ELSE '[]' END) je WHERE json_extract(je.value, '$[0]') = 't' LIMIT 1) ELSE NULL END) ON CONFLICT(id) DO UPDATE SET content=excluded.content, tags_json=excluded.tags_json, sig=excluded.sig, mentioned_pubkeys=excluded.mentioned_pubkeys, mentioned_hashtags=excluded.mentioned_hashtags, subject=excluded.subject, is_deleted=excluded.is_deleted, freenet_key=excluded.freenet_key, is_freenet_native=excluded.is_freenet_native, rsvp_event_id=excluded.rsvp_event_id, category=excluded.category";
const POST_FEED_SQL: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id FROM posts WHERE pubkey IN (SELECT value FROM json_each(?1)) AND is_deleted = 0 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3";
const POST_SELECT_BY_ID: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id FROM posts WHERE id = ?1";
const POST_SELECT_BY_PUBKEY: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id FROM posts WHERE pubkey = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3";
const POST_SELECT_REPLIES: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id FROM posts WHERE (root_id = ?1 OR id = ?1) AND is_deleted = 0 ORDER BY created_at ASC";
const POST_SELECT_PAGED: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id FROM posts WHERE is_deleted = 0 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2";
/// Slim feed variant: only the columns the feed surface consumes. Feed pages
/// are the hottest read path; the 17-column row mapping wastes decode work
/// on sig/mention/freenet/scheduling columns the UI never sees.
const POST_SELECT_PAGED_META: &str =
    "SELECT id, pubkey, content, created_at, tags_json FROM posts WHERE is_deleted = 0 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2";
const POST_SELECT_PAGED_META_CURSOR: &str =
    "SELECT id, pubkey, content, created_at, tags_json FROM posts WHERE is_deleted = 0 AND created_at < ?1 ORDER BY created_at DESC LIMIT ?2";
const POST_SELECT_SCHEDULED: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native FROM posts WHERE pubkey = ?1 AND scheduled_at IS NOT NULL AND is_deleted = 0 ORDER BY scheduled_at ASC";

pub struct PostRepo<'a> {
    db: &'a Database,
}

impl<'a> PostRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<PostRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(&conn, POST_SELECT_BY_ID, params![id], Self::map_row)
    }

    /// Fetch many rows by id in a single indexed `IN (json_each)` query.
    /// Missing ids are simply absent from the result.
    pub fn get_by_ids(&self, ids: &[String]) -> Result<Vec<PostRow>, crate::error::DbError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids_json = serde_json::to_string(ids)
            .map_err(|e| crate::error::DbError::Migration(e.to_string()))?;
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native, rsvp_event_id FROM posts WHERE id IN (SELECT value FROM json_each(?1))",
            params![ids_json.as_str()],
            Self::map_row,
        )
    }

    pub fn get_user_posts(
        &self,
        pubkey: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PostRow>, crate::error::DbError> {
        let (limit, offset) = crate::repos::clamp_page(limit, offset);
        let conn = self.db.conn()?;
        crate::query::query_capacity(
            &conn,
            POST_SELECT_BY_PUBKEY,
            params![pubkey, limit, offset],
            limit as usize,
            Self::map_row,
        )
    }

    pub fn get_replies_for_root(
        &self,
        root_id: &str,
    ) -> Result<Vec<PostRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(&conn, POST_SELECT_REPLIES, params![root_id], Self::map_row)
    }

    pub fn upsert(&self, post: &PostRow) -> Result<(), crate::error::DbError> {
        if limits::row_too_big(&post.content, &post.tags_json) {
            return Err(crate::error::DbError::Oversized(format!(
                "post {} exceeds relay size caps (content > {} or total > {} bytes)",
                post.id,
                limits::MAX_CONTENT_BYTES,
                limits::MAX_BATCH_BYTES
            )));
        }
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            POST_UPSERT_SQL,
            params![
                post.id.as_str(),
                post.pubkey.as_str(),
                post.content.as_str(),
                post.kind,
                post.created_at,
                post.tags_json.as_str(),
                post.sig.as_deref(),
                post.reply_to.as_deref(),
                post.root_id.as_deref(),
                post.mentioned_pubkeys.as_str(),
                post.mentioned_hashtags.as_str(),
                post.subject.as_deref(),
                post.sync_status.as_str(),
                post.is_deleted,
                post.scheduled_at,
                post.freenet_key.as_deref(),
                post.is_freenet_native,
                post.rsvp_event_id.as_deref(),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_batch(&self, posts: &[PostRow]) -> Result<(), crate::error::DbError> {
        if posts.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_batch_in(&tx, posts).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        post: &PostRow,
    ) -> Result<(), crate::error::DbError> {
        if limits::row_too_big(&post.content, &post.tags_json) {
            return Ok(());
        }
        tx.execute(
            POST_UPSERT_SQL,
            params![
                post.id.as_str(),
                post.pubkey.as_str(),
                post.content.as_str(),
                post.kind,
                post.created_at,
                post.tags_json.as_str(),
                post.sig.as_deref(),
                post.reply_to.as_deref(),
                post.root_id.as_deref(),
                post.mentioned_pubkeys.as_str(),
                post.mentioned_hashtags.as_str(),
                post.subject.as_deref(),
                post.sync_status.as_str(),
                post.is_deleted,
                post.scheduled_at,
                post.freenet_key.as_deref(),
                post.is_freenet_native,
                post.rsvp_event_id.as_deref(),
            ],
        )
        .await?;
        Ok(())
    }

    pub async fn upsert_batch_in(
        &self,
        tx: &libsql::Transaction,
        posts: &[PostRow],
    ) -> Result<(), crate::error::DbError> {
        let mut stmt = tx.prepare(POST_UPSERT_SQL).await?;
        for post in posts {
            if limits::row_too_big(&post.content, &post.tags_json) {
                continue; // relay content too large: skip, never store
            }
            stmt.run(params![
                post.id.as_str(),
                post.pubkey.as_str(),
                post.content.as_str(),
                post.kind,
                post.created_at,
                post.tags_json.as_str(),
                post.sig.as_deref(),
                post.reply_to.as_deref(),
                post.root_id.as_deref(),
                post.mentioned_pubkeys.as_str(),
                post.mentioned_hashtags.as_str(),
                post.subject.as_deref(),
                post.sync_status.as_str(),
                post.is_deleted,
                post.scheduled_at,
                post.freenet_key.as_deref(),
                post.is_freenet_native,
                post.rsvp_event_id.as_deref(),
            ])
            .await?;
            stmt.reset();
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE posts SET is_deleted = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Soft-deletes all non-deleted posts older than `cutoff_secs` (unix time);
    /// returns the number of rows affected.
    pub fn delete_older_than(&self, cutoff_secs: i64) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0 AND created_at < ?1",
            params![cutoff_secs],
        )
    }

    /// Soft-deletes every cached post; returns the number of rows affected.
    pub fn delete_all_posts(&self) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0",
            (),
        )
    }

    pub fn get_feed(
        &self,
        pubkeys: &[String],
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PostRow>, crate::error::DbError> {
        if pubkeys.is_empty() {
            return Ok(vec![]);
        }
        let (limit, offset) = crate::repos::clamp_page(limit, offset);
        let pubkeys_json = serde_json::to_string(pubkeys).unwrap_or_else(|_| "[]".into());
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            POST_FEED_SQL,
            params![pubkeys_json, limit, offset],
            Self::map_row,
        )
    }

    pub fn get_recent(&self, limit: i64) -> Result<Vec<PostRow>, crate::error::DbError> {
        self.get_paged(limit, 0)
    }

    pub fn get_paged(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PostRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let offset = offset.max(0);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            POST_SELECT_PAGED,
            params![limit, offset],
            Self::map_row,
        )
    }

    /// Slim paged fetch for feed rendering: id, pubkey, content, created_at,
    /// tags_json only (see `POST_SELECT_PAGED_META`).
    pub fn get_paged_meta(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PostMetaRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let offset = offset.max(0);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            POST_SELECT_PAGED_META,
            params![limit, offset],
            |row| {
                Ok(PostMetaRow {
                    id: row.get(0)?,
                    pubkey: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                    tags_json: row.get(4)?,
                })
            },
        )
    }

    /// Slim cursor-based fetch for feed rendering: id, pubkey, content, created_at,
    /// tags_json only (see `POST_SELECT_PAGED_META_CURSOR`).
    pub fn get_paged_meta_cursor(
        &self,
        before_created_at: i64,
        limit: i64,
    ) -> Result<Vec<PostMetaRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            POST_SELECT_PAGED_META_CURSOR,
            params![before_created_at, limit],
            |row| {
                Ok(PostMetaRow {
                    id: row.get(0)?,
                    pubkey: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                    tags_json: row.get(4)?,
                })
            },
        )
    }

    pub fn get_scheduled(&self, pubkey: &str) -> Result<Vec<PostRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(&conn, POST_SELECT_SCHEDULED, params![pubkey], Self::map_row)
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<PostRow> {
        Ok(PostRow {
            id: row.get(0)?,
            pubkey: row.get(1)?,
            content: row.get(2)?,
            kind: row.get(3)?,
            created_at: row.get(4)?,
            tags_json: row.get(5)?,
            sig: row.get(6)?,
            reply_to: row.get(7)?,
            root_id: row.get(8)?,
            mentioned_pubkeys: row.get(9)?,
            mentioned_hashtags: row.get(10)?,
            subject: row.get(11)?,
            sync_status: row.get(12)?,
            is_deleted: row.get(13)?,
            scheduled_at: row.get(14)?,
            freenet_key: row.get(15)?,
            is_freenet_native: row.get(16)?,
            rsvp_event_id: row.get(17)?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PostRow {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub kind: i64,
    pub created_at: i64,
    pub tags_json: String,
    pub sig: Option<String>,
    pub reply_to: Option<String>,
    pub root_id: Option<String>,
    pub mentioned_pubkeys: String,
    pub mentioned_hashtags: String,
    pub subject: Option<String>,
    pub sync_status: String,
    pub is_deleted: bool,
    pub scheduled_at: Option<i64>,
    pub freenet_key: Option<String>,
    pub is_freenet_native: bool,
    pub rsvp_event_id: Option<String>,
}

/// Slim feed row: the five columns the feed FFI surface consumes.
pub struct PostMetaRow {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: i64,
    pub tags_json: String,
}
