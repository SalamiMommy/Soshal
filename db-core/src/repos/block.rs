use crate::Database;
use libsql::params;

pub struct BlockRepo<'a> {
    db: &'a Database,
}

impl<'a> BlockRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, b: &BlockRow) -> Result<(), crate::error::DbError> {
        let norm_pk = b.pubkey.trim();
        let norm_blocked = b.blocked_pubkey.trim();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM blocks WHERE LOWER(pubkey)=LOWER(?1) AND LOWER(blocked_pubkey)=LOWER(?2)",
            params![norm_pk, norm_blocked],
        )?;
        crate::query::execute(
            &conn,
            "INSERT INTO blocks (pubkey, blocked_pubkey, created_at) VALUES (?1,?2,?3) ON CONFLICT(pubkey,blocked_pubkey) DO NOTHING",
            params![norm_pk, norm_blocked, b.created_at],
        )?;
        Ok(())
    }

    pub fn is_blocked(&self, pubkey: &str, blocked: &str) -> Result<bool, crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let norm_blocked = blocked.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM blocks WHERE LOWER(pubkey)=LOWER(?1) AND LOWER(blocked_pubkey)=LOWER(?2)",
            params![norm_pk.as_str(), norm_blocked.as_str()],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn delete(&self, pubkey: &str, blocked_pubkey: &str) -> Result<(), crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let norm_blocked = blocked_pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM blocks WHERE LOWER(pubkey)=LOWER(?1) AND LOWER(blocked_pubkey)=LOWER(?2)",
            params![norm_pk.as_str(), norm_blocked.as_str()],
        )?;
        Ok(())
    }

    pub fn list(&self, pubkey: &str) -> Result<Vec<String>, crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT blocked_pubkey FROM blocks WHERE LOWER(pubkey)=LOWER(?1) ORDER BY created_at DESC",
            params![norm_pk.as_str()],
            |r| r.get::<String>(0),
        )
    }

    pub fn delete_all_for(&self, pubkey: &str) -> Result<(), crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM blocks WHERE LOWER(pubkey)=LOWER(?1)",
            params![norm_pk.as_str()],
        )?;
        Ok(())
    }
}

pub struct BlockRow {
    pub pubkey: String,
    pub blocked_pubkey: String,
    pub created_at: i64,
}
