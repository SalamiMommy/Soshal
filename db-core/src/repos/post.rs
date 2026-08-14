use crate::repos::limits;
use crate::Database;
use libsql::params;

const POST_SELECT: &str = "SELECT id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native FROM posts";
const POST_INSERT_COLUMNS: &str = "id, pubkey, content, kind, created_at, tags_json, sig, reply_to, root_id, mentioned_pubkeys, mentioned_hashtags, subject, sync_status, is_deleted, scheduled_at, freenet_key, is_freenet_native";

pub struct PostRepo<'a> {
    db: &'a Database,
}

impl<'a> PostRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<PostRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            &format!("{} WHERE id = ?1", POST_SELECT),
            params![id],
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
            &format!(
                "{} WHERE pubkey = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                POST_SELECT
            ),
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
        crate::query::query(
            &conn,
            &format!(
                "{} WHERE (root_id = ?1 OR id = ?1) AND is_deleted = 0 ORDER BY created_at ASC",
                POST_SELECT
            ),
            params![root_id],
            Self::map_row,
        )
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
            &format!(
                "INSERT INTO posts ({}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17) ON CONFLICT(id) DO UPDATE SET content=excluded.content, tags_json=excluded.tags_json, sig=excluded.sig, mentioned_pubkeys=excluded.mentioned_pubkeys, mentioned_hashtags=excluded.mentioned_hashtags, subject=excluded.subject, is_deleted=excluded.is_deleted, freenet_key=excluded.freenet_key, is_freenet_native=excluded.is_freenet_native",
                POST_INSERT_COLUMNS,
            ),
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
            let sql = format!("INSERT INTO posts ({}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17) ON CONFLICT(id) DO UPDATE SET content=excluded.content, tags_json=excluded.tags_json, sig=excluded.sig, mentioned_pubkeys=excluded.mentioned_pubkeys, mentioned_hashtags=excluded.mentioned_hashtags, subject=excluded.subject, is_deleted=excluded.is_deleted, freenet_key=excluded.freenet_key, is_freenet_native=excluded.is_freenet_native", POST_INSERT_COLUMNS);
            for post in posts {
                if limits::row_too_big(&post.content, &post.tags_json) {
                    continue; // relay content too large: skip, never store
                }
                tx.execute(
                    &sql,
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
                    ],
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
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
            &format!(
                "{} WHERE pubkey IN (SELECT value FROM json_each(?1)) AND is_deleted = 0 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                POST_SELECT,
            ),
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
            &format!(
                "{} WHERE is_deleted = 0 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                POST_SELECT
            ),
            params![limit, offset],
            Self::map_row,
        )
    }

    pub fn get_scheduled(&self, pubkey: &str) -> Result<Vec<PostRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            &format!(
                "{} WHERE pubkey = ?1 AND scheduled_at IS NOT NULL AND is_deleted = 0 ORDER BY scheduled_at ASC",
                POST_SELECT
            ),
            params![pubkey],
            Self::map_row,
        )
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
}
