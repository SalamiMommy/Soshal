use crate::Database;
use libsql::params;

/// Persistent notification-ignore store (ignore-by-user and ignore-by-thread).
///
/// Mirrors `conversation_mute` so a user's Ignore decisions survive restarts and
/// are not replayed by the next DB fetch.
pub struct IgnoredNotificationRepo<'a> {
    db: &'a Database,
}

impl<'a> IgnoredNotificationRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Ignore all notifications from a user (event_id left empty).
    pub fn ignore_user(
        &self,
        user_pubkey: &str,
        from_pubkey: &str,
        kind: &str,
        at: i64,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        execute_ignore(&conn, user_pubkey, kind, from_pubkey, "", at)?;
        Ok(())
    }

    /// Ignore/turn-off notifications for a thread/post (from_pubkey left empty).
    pub fn ignore_thread(
        &self,
        user_pubkey: &str,
        event_id: &str,
        kind: &str,
        at: i64,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        execute_ignore(&conn, user_pubkey, kind, "", event_id, at)?;
        Ok(())
    }

    /// Remove an ignore-by-user decision.
    pub fn unignore_user(
        &self,
        user_pubkey: &str,
        from_pubkey: &str,
        kind: &str,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        delete_ignore(&conn, user_pubkey, kind, from_pubkey, "")?;
        Ok(())
    }

    /// Remove an ignore-by-thread decision.
    pub fn unignore_thread(
        &self,
        user_pubkey: &str,
        event_id: &str,
        kind: &str,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        delete_ignore(&conn, user_pubkey, kind, "", event_id)?;
        Ok(())
    }

    /// Whether a notification from `from_pubkey` about `event_id` is ignored.
    /// A user-level ignore wins even when a specific event is queried.
    pub fn is_ignored(
        &self,
        user_pubkey: &str,
        kind: &str,
        from_pubkey: &str,
        event_id: &str,
    ) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM ignored_notifications
             WHERE pubkey=?1 AND kind=?2 AND (
                 (from_pubkey=?3 AND event_id='')
              OR (event_id=?4 AND from_pubkey='')
              OR (event_id=?4 AND from_pubkey=?3)
             ) LIMIT 1",
            params![user_pubkey, kind, from_pubkey, event_id],
            |_| Ok(true),
        )?
        .is_some())
    }

    /// List all ignore rows for a user (for an Ignored List dashboard).
    pub fn list(
        &self,
        user_pubkey: &str,
    ) -> Result<Vec<(String, String, String, i64)>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT kind, from_pubkey, event_id, created_at
             FROM ignored_notifications WHERE pubkey=?1 ORDER BY created_at DESC",
            params![user_pubkey],
            |r| {
                Ok((
                    r.get::<String>(0)?,
                    r.get::<String>(1)?,
                    r.get::<String>(2)?,
                    r.get::<i64>(3)?,
                ))
            },
        )
    }
}

fn execute_ignore(
    conn: &libsql::Connection,
    user_pubkey: &str,
    kind: &str,
    from_pubkey: &str,
    event_id: &str,
    at: i64,
) -> Result<(), crate::error::DbError> {
    crate::query::execute(
        conn,
        "INSERT INTO ignored_notifications (pubkey, kind, from_pubkey, event_id, created_at)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(pubkey, kind, from_pubkey, event_id) DO NOTHING",
        params![user_pubkey, kind, from_pubkey, event_id, at],
    )?;
    Ok(())
}

fn delete_ignore(
    conn: &libsql::Connection,
    user_pubkey: &str,
    kind: &str,
    from_pubkey: &str,
    event_id: &str,
) -> Result<(), crate::error::DbError> {
    crate::query::execute(
        conn,
        "DELETE FROM ignored_notifications
         WHERE pubkey=?1 AND kind=?2 AND from_pubkey=?3 AND event_id=?4",
        params![user_pubkey, kind, from_pubkey, event_id],
    )?;
    Ok(())
}
