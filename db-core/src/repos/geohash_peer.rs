use crate::Database;
use libsql::params;

pub struct GeohashPeerRepo<'a> {
    db: &'a Database,
}

impl<'a> GeohashPeerRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, p: &GeohashPeerRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO geohash_peers (pubkey, geohash, purpose, first_seen, last_seen) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(pubkey) DO UPDATE SET geohash=excluded.geohash, purpose=excluded.purpose, last_seen=excluded.last_seen",
            params![p.pubkey.as_str(), p.geohash.as_str(), p.purpose.as_str(), p.first_seen, p.last_seen],
        )?;
        Ok(())
    }

    pub fn list_by_geohash(
        &self,
        geohash: &str,
    ) -> Result<Vec<GeohashPeerRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT pubkey, geohash, purpose, first_seen, last_seen FROM geohash_peers WHERE geohash=?1 ORDER BY last_seen DESC LIMIT 1000",
            params![geohash],
            Self::map_row,
        )
    }

    pub fn list_by_purpose(
        &self,
        purpose: &str,
    ) -> Result<Vec<GeohashPeerRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT pubkey, geohash, purpose, first_seen, last_seen FROM geohash_peers WHERE purpose=?1 OR purpose='both' ORDER BY last_seen DESC LIMIT 1000",
            params![purpose],
            Self::map_row,
        )
    }

    pub fn delete(&self, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM geohash_peers WHERE pubkey=?1",
            params![pubkey],
        )?;
        Ok(())
    }

    /// Drop peers unseen since `cutoff_secs_ago`.
    pub fn purge_stale(&self, cutoff_secs_ago: i64) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM geohash_peers WHERE last_seen < ?1",
            params![soshal_common_core::format::now_secs() - cutoff_secs_ago],
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<GeohashPeerRow> {
        Ok(GeohashPeerRow {
            pubkey: r.get(0)?,
            geohash: r.get(1)?,
            purpose: r.get(2)?,
            first_seen: r.get(3)?,
            last_seen: r.get(4)?,
        })
    }
}

pub struct GeohashPeerRow {
    pub pubkey: String,
    pub geohash: String,
    pub purpose: String,
    pub first_seen: i64,
    pub last_seen: i64,
}
