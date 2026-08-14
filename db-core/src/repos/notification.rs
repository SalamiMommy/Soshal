use crate::Database;
use libsql::params;

pub struct NotificationRepo<'a> {
    db: &'a Database,
}

impl<'a> NotificationRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, n: &NotificationRow) -> Result<(), crate::error::DbError> {
        if crate::repos::limits::notification_too_big(n.content.as_deref().unwrap_or("")) {
            return Err(crate::error::DbError::Oversized(format!(
                "notification exceeds {} byte cap",
                crate::repos::limits::MAX_NOTIFICATION_BYTES
            )));
        }
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO notifications (id, pubkey, type, event_id, from_pubkey, content, created_at, is_read) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET is_read=excluded.is_read",
            params![
                n.id.as_str(),
                n.pubkey.as_str(),
                n.type_.as_str(),
                n.event_id.as_deref(),
                n.from_pubkey.as_deref(),
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
            let sql = "INSERT INTO notifications (id, pubkey, type, event_id, from_pubkey, content, created_at, is_read) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET is_read=excluded.is_read";
            for n in notifications {
                if crate::repos::limits::notification_too_big(n.content.as_deref().unwrap_or("")) {
                    continue; // oversized notification payload: skip
                }
                tx.execute(
                    sql,
                    params![
                        n.id.as_str(),
                        n.pubkey.as_str(),
                        n.type_.as_str(),
                        n.event_id.as_deref(),
                        n.from_pubkey.as_deref(),
                        n.content.as_deref(),
                        n.created_at,
                        n.is_read,
                    ],
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn get_unread(
        &self,
        pubkey: &str,
        limit: i64,
    ) -> Result<Vec<NotificationRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, type, event_id, from_pubkey, content, created_at, is_read FROM notifications WHERE pubkey = ?1 AND is_read = 0 ORDER BY created_at DESC LIMIT ?2",
            params![pubkey, limit],
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
}

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
