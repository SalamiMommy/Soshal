use libsql::params;
use serde::{Deserialize, Serialize};

use crate::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupThreadRow {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub body: String,
    pub author: String,
    pub created_at: i64,
    pub is_pinned: bool,
    pub reply_count: i64,
    pub reaction_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupThreadReplyRow {
    pub id: String,
    pub thread_id: String,
    pub parent_id: String,
    pub author: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadReactionSummaryRow {
    pub thread_id: String,
    pub reply_id: String,
    pub emoji: String,
    pub count: i64,
    pub reacted: bool,
}

/// Thread list sort modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadSort {
    /// Pinned first, then newest first.
    Newest,
    /// Pinned first, then hot score = (reactions + replies) / hours-since-created.
    Popular,
}

impl ThreadSort {
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("popular") {
            Self::Popular
        } else {
            Self::Newest
        }
    }
}

pub struct GroupThreadRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupThreadRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    fn row_from_columns(row: &libsql::Row) -> Result<GroupThreadRow, libsql::Error> {
        Ok(GroupThreadRow {
            id: row.get(0)?,
            group_id: row.get(1)?,
            title: row.get(2)?,
            body: row.get(3)?,
            author: row.get(4)?,
            created_at: row.get(5)?,
            is_pinned: row.get::<i64>(6)? != 0,
            reply_count: row.get(7)?,
            reaction_count: row.get(8)?,
        })
    }

    const COLUMNS: &'static str = "t.id, t.group_id, t.title, t.body, t.author, t.created_at, t.is_pinned,
            (SELECT COUNT(*) FROM group_thread_replies r WHERE r.thread_id = t.id) AS reply_count,
            (SELECT COUNT(*) FROM group_thread_reactions g WHERE g.thread_id = t.id) AS reaction_count";

    pub fn list(
        &self,
        group_id: &str,
        sort: ThreadSort,
    ) -> Result<Vec<GroupThreadRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        let order = match sort {
            ThreadSort::Newest => "t.is_pinned DESC, t.created_at DESC",
            // Hot: engagement velocity — (reactions + replies) per hour since
            // creation. Pinned always floats above the feed.
            ThreadSort::Popular => {
                "t.is_pinned DESC,
                 (1.0 * (SELECT COUNT(*) FROM group_thread_replies r WHERE r.thread_id = t.id)
                      + (SELECT COUNT(*) FROM group_thread_reactions g WHERE g.thread_id = t.id))
                 / MAX((?2 - t.created_at) / 3600.0, 0.1) DESC, t.created_at DESC"
            }
        };
        let sql = format!(
            "SELECT {} FROM group_threads t WHERE t.group_id = ?1 ORDER BY {}",
            Self::COLUMNS,
            order
        );
        if sort == ThreadSort::Popular {
            let now = soshal_common_core::format::now_secs();
            crate::query::query(&conn, &sql, params![group_id, now], |row| {
                Self::row_from_columns(row)
            })
        } else {
            crate::query::query(&conn, &sql, [group_id], Self::row_from_columns)
        }
    }

    pub fn get(&self, thread_id: &str) -> Result<Option<GroupThreadRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            &format!(
                "SELECT {} FROM group_threads t WHERE t.id = ?1",
                Self::COLUMNS
            ),
            [thread_id],
            Self::row_from_columns,
        )
    }

    pub fn upsert(&self, thread: &GroupThreadRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_threads (id, group_id, title, body, author, created_at, is_pinned, reply_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                thread.id.as_str(),
                thread.group_id.as_str(),
                thread.title.as_str(),
                thread.body.as_str(),
                thread.author.as_str(),
                thread.created_at,
                thread.is_pinned as i64,
                thread.reply_count
            ],
        )?;
        Ok(())
    }

    pub fn set_pinned(&self, thread_id: &str, pinned: bool) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE group_threads SET is_pinned = ?1 WHERE id = ?2",
            params![pinned as i64, thread_id],
        )?;
        Ok(())
    }

    pub fn delete(&self, thread_id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_threads WHERE id = ?1",
            [thread_id],
        )?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_thread_replies WHERE thread_id = ?1",
            [thread_id],
        )?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_thread_reactions WHERE thread_id = ?1",
            [thread_id],
        )?;
        Ok(())
    }

    pub fn add_reply(&self, reply: &GroupThreadReplyRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_thread_replies (id, thread_id, parent_id, author, content, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                reply.id.as_str(),
                reply.thread_id.as_str(),
                reply.parent_id.as_str(),
                reply.author.as_str(),
                reply.content.as_str(),
                reply.created_at
            ],
        )?;
        Ok(())
    }

    pub fn replies(
        &self,
        thread_id: &str,
    ) -> Result<Vec<GroupThreadReplyRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, thread_id, parent_id, author, content, created_at
             FROM group_thread_replies WHERE thread_id = ?1 ORDER BY created_at ASC",
            [thread_id],
            |row| {
                Ok(GroupThreadReplyRow {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    author: row.get(3)?,
                    content: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
    }

    pub fn delete_reply(&self, reply_id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_thread_replies WHERE id = ?1",
            [reply_id],
        )?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_thread_reactions WHERE reply_id = ?1",
            [reply_id],
        )?;
        Ok(())
    }

    /// Add a reaction (no-op when already present). Returns true if inserted.
    pub fn add_reaction(
        &self,
        thread_id: &str,
        reply_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO group_thread_reactions (thread_id, reply_id, pubkey, emoji, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![thread_id, reply_id, pubkey, emoji, soshal_common_core::format::now_secs()],
        )?;
        Ok(changed > 0)
    }

    /// Remove a reaction. Returns true when a row was removed.
    pub fn remove_reaction(
        &self,
        thread_id: &str,
        reply_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "DELETE FROM group_thread_reactions
             WHERE thread_id = ?1 AND reply_id = ?2 AND pubkey = ?3 AND emoji = ?4",
            params![thread_id, reply_id, pubkey, emoji],
        )?;
        Ok(changed > 0)
    }

    /// Toggle a reaction; returns true when it was added, false when removed.
    pub fn toggle_reaction(
        &self,
        thread_id: &str,
        reply_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<bool, crate::error::DbError> {
        if self.has_reaction(thread_id, reply_id, pubkey, emoji)? {
            self.remove_reaction(thread_id, reply_id, pubkey, emoji)?;
            Ok(false)
        } else {
            self.add_reaction(thread_id, reply_id, pubkey, emoji)?;
            Ok(true)
        }
    }

    pub fn has_reaction(
        &self,
        thread_id: &str,
        reply_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT 1 FROM group_thread_reactions
             WHERE thread_id = ?1 AND reply_id = ?2 AND pubkey = ?3 AND emoji = ?4",
            params![thread_id, reply_id, pubkey, emoji],
            |r| r.get::<i64>(0),
        )
        .map(|v| v.is_some())
    }

    /// Emoji reaction counts for a thread (and optionally one reply),
    /// each with whether `viewer_pubkey` reacted. Reply-level rows carry
    /// `reply_id = ''` for thread-level totals.
    pub fn reaction_summary(
        &self,
        thread_id: &str,
        viewer_pubkey: &str,
    ) -> Result<Vec<ThreadReactionSummaryRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT r.thread_id, r.reply_id, r.emoji, COUNT(*) AS cnt,
                    MAX(CASE WHEN r.pubkey = ?2 THEN 1 ELSE 0 END) AS reacted
             FROM group_thread_reactions r
             WHERE r.thread_id = ?1
             GROUP BY r.reply_id, r.emoji
             ORDER BY cnt DESC, r.emoji ASC",
            [thread_id, viewer_pubkey],
            |row| {
                Ok(ThreadReactionSummaryRow {
                    thread_id: row.get(0)?,
                    reply_id: row.get(1)?,
                    emoji: row.get(2)?,
                    count: row.get(3)?,
                    reacted: row.get::<i64>(4)? != 0,
                })
            },
        )
    }
}
