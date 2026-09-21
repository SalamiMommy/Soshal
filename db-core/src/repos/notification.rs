use crate::Database;
use libsql::params;

pub struct NotificationRepo<'a> {
    db: &'a Database,
}

impl<'a> NotificationRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, n: &NotificationRow) -> Result<(), crate::error::DbError> {
        if crate::repos::limits::notification_too_big(n.content.as_deref().unwrap_or("")) {
            return Err(crate::error::DbError::Oversized(format!(
                "notification exceeds {} byte cap",
                crate::repos::limits::MAX_NOTIFICATION_BYTES
            )));
        }
        let conn = self.db.conn()?;
        let norm_pk = n.pubkey.trim();
        crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![norm_pk],
        )?;
        let norm_event_id = n.event_id.as_deref().map(|s| s.trim().to_ascii_lowercase());
        let norm_from_pk = n
            .from_pubkey
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase());
        crate::query::execute(
            &conn,
            "INSERT INTO notifications (id, pubkey, type, event_id, from_pubkey, content, created_at, is_read) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET is_read = CASE WHEN notifications.is_read = 1 THEN 1 ELSE excluded.is_read END",
            params![
                n.id.as_str(),
                norm_pk,
                n.type_.as_str(),
                norm_event_id.as_deref(),
                norm_from_pk.as_deref(),
                n.content.as_deref(),
                n.created_at,
                n.is_read,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_batch(
        &self,
        notifications: &[NotificationRow],
    ) -> Result<(), crate::error::DbError> {
        if notifications.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_batch_in(&tx, notifications).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Transaction-scoped batch upsert (ingest path writes inside its own
    /// IMMEDIATE transaction). Same read-state-preserving conflict rule as
    /// `upsert`: a row already marked read stays read on replay.
    pub async fn upsert_batch_in(
        &self,
        tx: &libsql::Transaction,
        notifications: &[NotificationRow],
    ) -> Result<(), crate::error::DbError> {
        if notifications.is_empty() {
            return Ok(());
        }
        let sql = "INSERT INTO notifications (id, pubkey, type, event_id, from_pubkey, content, created_at, is_read) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET is_read = CASE WHEN notifications.is_read = 1 THEN 1 ELSE excluded.is_read END, content = CASE WHEN excluded.created_at > notifications.created_at THEN excluded.content ELSE notifications.content END, created_at = CASE WHEN excluded.created_at > notifications.created_at THEN excluded.created_at ELSE notifications.created_at END, event_id = CASE WHEN excluded.created_at > notifications.created_at THEN excluded.event_id ELSE notifications.event_id END";
        let stmt = tx.prepare(sql).await?;
        for n in notifications {
            if crate::repos::limits::notification_too_big(n.content.as_deref().unwrap_or("")) {
                continue; // oversized notification payload: skip
            }
            let norm_pk = n.pubkey.trim();
            let norm_event_id = n.event_id.as_deref().map(|s| s.trim().to_ascii_lowercase());
            let norm_from_pk = n
                .from_pubkey
                .as_deref()
                .map(|s| s.trim().to_ascii_lowercase());
            tx.execute(
                "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
                params![norm_pk],
            )
            .await?;
            stmt.run(params![
                n.id.as_str(),
                norm_pk,
                n.type_.as_str(),
                norm_event_id.as_deref(),
                norm_from_pk.as_deref(),
                n.content.as_deref(),
                n.created_at,
                n.is_read,
            ])
            .await?;
            stmt.reset();
        }
        Ok(())
    }

    pub fn get_unread(
        &self,
        pubkey: &str,
        limit: i64,
    ) -> Result<Vec<NotificationRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        crate::query::query(
            &conn,
            "SELECT id, pubkey, type, event_id, from_pubkey, content, created_at, is_read FROM notifications WHERE LOWER(pubkey) = LOWER(?1) AND is_read = 0 AND NOT EXISTS (SELECT 1 FROM ignored_notifications i WHERE LOWER(i.pubkey) = LOWER(notifications.pubkey) AND (i.kind = notifications.type OR i.kind = 'user' OR i.kind = 'thread' OR i.kind = 'all') AND ((LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND i.event_id = '') OR (LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, '')) AND i.from_pubkey = '') OR (LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, ''))))) ORDER BY created_at DESC LIMIT ?2",
            params![norm_pk.as_str(), limit],
            |row| {
                Ok(NotificationRow {
                    id: row.get(0)?,
                    pubkey: row.get(1)?,
                    type_: row.get(2)?,
                    event_id: row.get(3)?,
                    from_pubkey: row.get(4)?,
                    content: row.get(5)?,
                    created_at: row.get(6)?,
                    is_read: row.get(7)?,
                })
            },
        )
    }

    pub fn get_unread_filtered(
        &self,
        pubkey: &str,
        type_: &str,
        limit: i64,
    ) -> Result<Vec<NotificationRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        crate::query::query(
            &conn,
            "SELECT id, pubkey, type, event_id, from_pubkey, content, created_at, is_read FROM notifications WHERE LOWER(pubkey) = LOWER(?1) AND is_read = 0 AND type = ?2 AND NOT EXISTS (SELECT 1 FROM ignored_notifications i WHERE LOWER(i.pubkey) = LOWER(notifications.pubkey) AND (i.kind = notifications.type OR i.kind = 'user' OR i.kind = 'thread' OR i.kind = 'all') AND ((LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND i.event_id = '') OR (LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, '')) AND i.from_pubkey = '') OR (LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, ''))))) ORDER BY created_at DESC LIMIT ?3",
            params![norm_pk.as_str(), type_, limit],
            |row| {
                Ok(NotificationRow {
                    id: row.get(0)?,
                    pubkey: row.get(1)?,
                    type_: row.get(2)?,
                    event_id: row.get(3)?,
                    from_pubkey: row.get(4)?,
                    content: row.get(5)?,
                    created_at: row.get(6)?,
                    is_read: row.get(7)?,
                })
            },
        )
    }

    pub fn count_unread(&self, pubkey: &str) -> Result<i64, crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let count: i64 = crate::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM notifications WHERE LOWER(pubkey) = LOWER(?1) AND is_read = 0 AND NOT EXISTS (SELECT 1 FROM ignored_notifications i WHERE LOWER(i.pubkey) = LOWER(notifications.pubkey) AND (i.kind = notifications.type OR i.kind = 'user' OR i.kind = 'thread' OR i.kind = 'all') AND ((LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND i.event_id = '') OR (LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, '')) AND i.from_pubkey = '') OR (LOWER(i.from_pubkey) = LOWER(COALESCE(notifications.from_pubkey, '')) AND LOWER(i.event_id) = LOWER(COALESCE(notifications.event_id, '')))))",
            params![norm_pk.as_str()],
            |row| row.get(0),
        )?
        .unwrap_or(0);
        Ok(count)
    }

    pub fn mark_as_read(&self, pubkey: &str, id: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let affected = crate::query::execute(
            &conn,
            "UPDATE notifications SET is_read = 1 WHERE id = ?1 AND LOWER(pubkey) = ?2 AND is_read = 0",
            params![id.trim(), norm_pk.as_str()],
        )?;
        Ok(affected > 0)
    }

    pub fn mark_all_read(&self, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        crate::query::execute(
            &conn,
            "UPDATE notifications SET is_read = 1 WHERE LOWER(pubkey) = ?1",
            params![norm_pk.as_str()],
        )?;
        Ok(())
    }

    pub fn delete(&self, pubkey: &str, id: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let affected = crate::query::execute(
            &conn,
            "DELETE FROM notifications WHERE id = ?1 AND LOWER(pubkey) = ?2",
            params![id.trim(), norm_pk.as_str()],
        )?;
        Ok(affected > 0)
    }
}

#[derive(Debug, Clone)]
pub struct NotificationRow {
    pub id: String,
    pub pubkey: String,
    pub type_: String,
    pub event_id: Option<String>,
    pub from_pubkey: Option<String>,
    pub content: Option<String>,
    pub created_at: i64,
    pub is_read: bool,
}
