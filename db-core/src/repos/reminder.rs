use libsql::params;
use serde::{Deserialize, Serialize};

use crate::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderRow {
    pub id: String,
    pub event_id: String,
    pub title: String,
    pub start_time: i64,
    pub minutes_before: i64,
    pub created_at: i64,
}

pub struct ReminderRepo<'a> {
    db: &'a Database,
}

impl<'a> ReminderRepo<'a> {
    soshal_repo_new!();

    pub fn list(&self) -> Result<Vec<ReminderRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, event_id, title, start_time, minutes_before, created_at
             FROM reminders ORDER BY start_time ASC LIMIT 1000",
            (),
            |row| {
                Ok(ReminderRow {
                    id: row.get(0)?,
                    event_id: row.get(1)?,
                    title: row.get(2)?,
                    start_time: row.get(3)?,
                    minutes_before: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
    }

    pub fn upsert(&self, r: &ReminderRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO reminders (id, event_id, title, start_time, minutes_before, created_at, trigger_at)
             VALUES (?1,?2,?3,?4,?5,?6, ?4 - ?5 * 60)",
            params![r.id.as_str(), r.event_id.as_str(), r.title.as_str(), r.start_time, r.minutes_before, r.created_at],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM reminders WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Reminders whose trigger time (start - minutes_before) is within the
    /// given window [now, now + lookahead_ms], for notification firing.
    pub fn due(
        &self,
        now: i64,
        lookahead_ms: i64,
    ) -> Result<Vec<ReminderRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        let window_start = now;
        let window_end = now + lookahead_ms / 1000;
        crate::query::query(
            &conn,
            "SELECT id, event_id, title, start_time, minutes_before, created_at
             FROM reminders
             WHERE trigger_at >= ?1
               AND trigger_at <= ?2
             ORDER BY trigger_at ASC",
            params![window_start, window_end],
            |row| {
                Ok(ReminderRow {
                    id: row.get(0)?,
                    event_id: row.get(1)?,
                    title: row.get(2)?,
                    start_time: row.get(3)?,
                    minutes_before: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
    }
}
