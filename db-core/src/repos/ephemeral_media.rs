use crate::Database;
use libsql::params;
use serde::Serialize;

/// Row for the `ephemeral_media` table. Tracks disappearing DM media with a
/// view-count cap and an optional expiry. State follows the lifecycle:
/// `pending → expired | screenshot_detected`.
#[derive(Serialize)]
pub struct EphemeralMediaRow {
    pub id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub conversation_type: String,
    pub media_url: String,
    pub media_type: String,
    pub sender_pubkey: String,
    pub recipient_pubkey: String,
    pub max_views: i64,
    pub current_views: i64,
    pub state: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub viewed_at: Option<i64>,
}

pub struct EphemeralMediaRepo<'a> {
    db: &'a Database,
}

impl<'a> EphemeralMediaRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn create(&self, e: &EphemeralMediaRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO ephemeral_media
             (id, message_id, conversation_id, conversation_type, media_url, media_type, sender_pubkey, recipient_pubkey, max_views, current_views, state, expires_at, created_at, viewed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                e.id.as_str(),
                e.message_id.as_str(),
                e.conversation_id.as_str(),
                e.conversation_type.as_str(),
                e.media_url.as_str(),
                e.media_type.as_str(),
                e.sender_pubkey.as_str(),
                e.recipient_pubkey.as_str(),
                e.max_views,
                e.current_views,
                e.state.as_str(),
                e.expires_at,
                e.created_at,
                e.viewed_at,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<EphemeralMediaRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, message_id, conversation_id, conversation_type, media_url, media_type, sender_pubkey, recipient_pubkey, max_views, current_views, state, expires_at, created_at, viewed_at
             FROM ephemeral_media WHERE id=?1",
            params![id],
            row_to_ephemeral_media,
        )
    }

    pub fn get_by_message_id(
        &self,
        message_id: &str,
    ) -> Result<Option<EphemeralMediaRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, message_id, conversation_id, conversation_type, media_url, media_type, sender_pubkey, recipient_pubkey, max_views, current_views, state, expires_at, created_at, viewed_at
             FROM ephemeral_media WHERE message_id=?1",
            params![message_id],
            row_to_ephemeral_media,
        )
    }

    /// All pending media addressed to `pubkey`, newest first.
    pub fn get_pending_for_recipient(
        &self,
        pubkey: &str,
    ) -> Result<Vec<EphemeralMediaRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, message_id, conversation_id, conversation_type, media_url, media_type, sender_pubkey, recipient_pubkey, max_views, current_views, state, expires_at, created_at, viewed_at
             FROM ephemeral_media WHERE recipient_pubkey=?1 AND state='pending' ORDER BY created_at DESC",
            params![pubkey],
            row_to_ephemeral_media,
        )
    }

    /// Increments the view counter and stamps `viewed_at`; flips state to
    /// `expired` in the same UPDATE once `current_views` reaches `max_views`.
    /// Returns the fresh row, or `None` when the id doesn't exist.
    pub fn increment_view_count(
        &self,
        id: &str,
    ) -> Result<Option<EphemeralMediaRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "UPDATE ephemeral_media
             SET current_views = current_views + 1,
                 viewed_at = ?2,
                 state = CASE WHEN current_views + 1 >= max_views THEN 'expired' ELSE state END
             WHERE id=?1
             RETURNING id, message_id, conversation_id, conversation_type, media_url, media_type, sender_pubkey, recipient_pubkey, max_views, current_views, state, expires_at, created_at, viewed_at",
            params![id, soshal_common_core::format::now_secs()],
            row_to_ephemeral_media,
        )
    }

    pub fn mark_state(&self, id: &str, state: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let changed = crate::query::execute(
            &conn,
            "UPDATE ephemeral_media SET state=?2 WHERE id=?1",
            params![id, state],
        )?;
        if changed == 0 {
            return Err(crate::error::DbError::NotFound);
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM ephemeral_media WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    /// Deletes rows whose `expires_at` is in the past; returns the removed ids.
    pub fn clean_expired(&self, now: i64) -> Result<Vec<String>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let stmt = tx
                .prepare(
                    "SELECT id FROM ephemeral_media WHERE expires_at IS NOT NULL AND expires_at < ?1",
                )
                .await?;
            let mut rows = stmt.query(params![now]).await?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next().await? {
                ids.push(row.get::<String>(0)?);
            }
            if !ids.is_empty() {
                tx.execute(
                    "DELETE FROM ephemeral_media WHERE expires_at < ?1",
                    params![now],
                )
                .await?;
            }
            tx.commit().await?;
            Ok(ids)
        })
    }
}

fn row_to_ephemeral_media(r: &libsql::Row) -> libsql::Result<EphemeralMediaRow> {
    Ok(EphemeralMediaRow {
        id: r.get(0)?,
        message_id: r.get(1)?,
        conversation_id: r.get(2)?,
        conversation_type: r.get(3)?,
        media_url: r.get(4)?,
        media_type: r.get(5)?,
        sender_pubkey: r.get(6)?,
        recipient_pubkey: r.get(7)?,
        max_views: r.get(8)?,
        current_views: r.get(9)?,
        state: r.get(10)?,
        expires_at: r.get(11)?,
        created_at: r.get(12)?,
        viewed_at: r.get(13)?,
    })
}
