use crate::error::DbError;
use crate::Database;
use libsql::{params, Row};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditLogRow {
    pub id: String,
    pub group_id: String,
    pub actor_pubkey: String,
    pub action: String,
    pub target_pubkey: Option<String>,
    pub details: Option<String>,
    pub created_at: i64,
}

pub struct AuditLogRepo<'a> {
    db: &'a Database,
}

impl<'a> AuditLogRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, row: &AuditLogRow) -> Result<(), DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO audit_logs (id, group_id, actor_pubkey, action, target_pubkey, details, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                row.id.as_str(),
                row.group_id.as_str(),
                row.actor_pubkey.as_str(),
                row.action.as_str(),
                row.target_pubkey.as_deref(),
                row.details.as_deref(),
                row.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_by_actor(
        &self,
        actor_pubkey: &str,
        limit: i64,
    ) -> Result<Vec<AuditLogRow>, DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, group_id, actor_pubkey, action, target_pubkey, details, created_at FROM audit_logs WHERE actor_pubkey = ?1 ORDER BY created_at DESC LIMIT ?2",
            params![actor_pubkey, limit],
            Self::map_row,
        )
    }

    pub fn get_all(&self, limit: i64) -> Result<Vec<AuditLogRow>, DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, group_id, actor_pubkey, action, target_pubkey, details, created_at FROM audit_logs ORDER BY created_at DESC LIMIT ?1",
            params![limit],
            Self::map_row,
        )
    }

    fn map_row(row: &Row) -> libsql::Result<AuditLogRow> {
        Ok(AuditLogRow {
            id: row.get(0)?,
            group_id: row.get(1)?,
            actor_pubkey: row.get(2)?,
            action: row.get(3)?,
            target_pubkey: row.get(4)?,
            details: row.get(5)?,
            created_at: row.get(6)?,
        })
    }
}
