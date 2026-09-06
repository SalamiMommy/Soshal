use crate::Database;
use libsql::params;

pub struct DiagnosticLogRepo<'a> {
    db: &'a Database,
}

impl<'a> DiagnosticLogRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, l: &DiagnosticLogRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO diagnostic_logs (id, level, service, method, message, created_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO NOTHING",
            params![l.id.as_str(), l.level.as_str(), l.service.as_str(), l.method.as_str(), l.message.as_str(), l.created_at],
        )?;
        Ok(())
    }

    pub fn list(
        &self,
        limit: i64,
        level: Option<&str>,
    ) -> Result<Vec<DiagnosticLogRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        match level {
            Some(lv) => crate::query::query(
                &conn,
                "SELECT id, level, service, method, message, created_at FROM diagnostic_logs WHERE level=?1 ORDER BY created_at DESC LIMIT ?2",
                params![lv, limit],
                Self::map_row,
            ),
            None => crate::query::query(
                &conn,
                "SELECT id, level, service, method, message, created_at FROM diagnostic_logs ORDER BY created_at DESC LIMIT ?1",
                params![limit],
                Self::map_row,
            ),
        }
    }

    /// Drop entries older than `cutoff_secs_ago`.
    pub fn purge_before(&self, cutoff_secs_ago: i64) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM diagnostic_logs WHERE created_at < ?1",
            params![soshal_common_core::format::now_secs() - cutoff_secs_ago],
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<DiagnosticLogRow> {
        Ok(DiagnosticLogRow {
            id: r.get(0)?,
            level: r.get(1)?,
            service: r.get(2)?,
            method: r.get(3)?,
            message: r.get(4)?,
            created_at: r.get(5)?,
        })
    }
}

pub struct DiagnosticLogRow {
    pub id: String,
    pub level: String,
    pub service: String,
    pub method: String,
    pub message: String,
    pub created_at: i64,
}
