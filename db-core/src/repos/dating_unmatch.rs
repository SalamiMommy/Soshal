use crate::Database;
use libsql::params;

pub struct DatingUnmatchRepo<'a> {
    db: &'a Database,
}

impl<'a> DatingUnmatchRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, actor: &str, pubkey: &str, at: i64) -> Result<(), crate::error::DbError> {
        let norm_actor = actor.trim().to_ascii_lowercase();
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO dating_unmatches (actor_pubkey, pubkey, unmatched_at) VALUES (?1,?2,?3) ON CONFLICT(actor_pubkey, pubkey) DO UPDATE SET unmatched_at=excluded.unmatched_at",
            params![norm_actor.as_str(), norm_pk.as_str(), at],
        )?;
        Ok(())
    }

    pub fn is_unmatched(&self, actor: &str, pubkey: &str) -> Result<bool, crate::error::DbError> {
        let norm_actor = actor.trim().to_ascii_lowercase();
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM dating_unmatches WHERE LOWER(actor_pubkey)=LOWER(?1) AND LOWER(pubkey)=LOWER(?2)",
            params![norm_actor.as_str(), norm_pk.as_str()],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn delete(&self, actor: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let norm_actor = actor.trim().to_ascii_lowercase();
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM dating_unmatches WHERE LOWER(actor_pubkey)=LOWER(?1) AND LOWER(pubkey)=LOWER(?2)",
            params![norm_actor.as_str(), norm_pk.as_str()],
        )?;
        Ok(())
    }
}
