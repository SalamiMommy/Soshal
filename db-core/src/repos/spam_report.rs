use crate::Database;
use libsql::params;

pub struct SpamReportRepo<'a> {
    db: &'a Database,
}

impl<'a> SpamReportRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, r: &SpamReportRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = r.pubkey.trim().to_ascii_lowercase();
        let norm_target = r
            .target_pubkey
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase());
        crate::query::execute(
            &conn,
            "INSERT INTO spam_reports (id, pubkey, target_id, target_pubkey, reason, tags, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO NOTHING",
            params![
                r.id.as_str(),
                norm_pk.as_str(),
                r.target_id.as_deref(),
                norm_target.as_deref(),
                r.reason.as_deref(),
                r.tags.as_str(),
                r.created_at
            ],
        )?;
        Ok(())
    }

    pub fn list_by_target(
        &self,
        target_pubkey: &str,
        limit: i64,
    ) -> Result<Vec<SpamReportRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        let norm_target = target_pubkey.trim().to_ascii_lowercase();
        crate::query::query(
            &conn,
            "SELECT id, pubkey, target_id, target_pubkey, reason, tags, created_at FROM spam_reports WHERE LOWER(target_pubkey)=?1 ORDER BY created_at DESC LIMIT ?2",
            params![norm_target.as_str(), limit],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM spam_reports WHERE id=?1", params![id])?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<SpamReportRow> {
        Ok(SpamReportRow {
            id: r.get(0)?,
            pubkey: r.get(1)?,
            target_id: r.get(2)?,
            target_pubkey: r.get(3)?,
            reason: r.get(4)?,
            tags: r.get(5)?,
            created_at: r.get(6)?,
        })
    }
}

pub struct SpamReportRow {
    pub id: String,
    pub pubkey: String,
    pub target_id: Option<String>,
    pub target_pubkey: Option<String>,
    pub reason: Option<String>,
    pub tags: String,
    pub created_at: i64,
}
