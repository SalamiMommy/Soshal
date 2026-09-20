use crate::Database;
use libsql::params;

pub struct HashtagRepo<'a> {
    db: &'a Database,
}

impl<'a> HashtagRepo<'a> {
    soshal_repo_new!();

    pub fn get_by_tag(
        &self,
        tag: &str,
        pubkey: &str,
    ) -> Result<Option<HashtagRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT tag, pubkey, last_used_at, count FROM hashtags WHERE tag = ?1 AND pubkey = ?2",
            params![tag, pubkey],
            Self::map_row,
        )
    }

    pub fn get_trending(&self, limit: i64) -> Result<Vec<HashtagRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT tag, pubkey, last_used_at, count FROM hashtags ORDER BY count DESC LIMIT ?1",
            params![limit],
            Self::map_row,
        )
    }

    pub fn upsert(&self, row: &HashtagRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO hashtags (tag, pubkey, last_used_at, count) VALUES (?1,?2,?3,?4) ON CONFLICT(tag, pubkey) DO UPDATE SET last_used_at=excluded.last_used_at, count=count+excluded.count",
            params![row.tag.as_str(), row.pubkey.as_str(), row.last_used_at, row.count],
        )?;
        Ok(())
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &HashtagRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "INSERT INTO hashtags (tag, pubkey, last_used_at, count) VALUES (?1,?2,?3,?4) ON CONFLICT(tag, pubkey) DO UPDATE SET last_used_at=excluded.last_used_at, count=count+excluded.count",
            params![row.tag.as_str(), row.pubkey.as_str(), row.last_used_at, row.count],
        )
        .await?;
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<HashtagRow> {
        Ok(HashtagRow {
            tag: row.get(0)?,
            pubkey: row.get(1)?,
            last_used_at: row.get(2)?,
            count: row.get(3)?,
        })
    }
}

pub struct HashtagRow {
    pub tag: String,
    pub pubkey: String,
    pub last_used_at: i64,
    pub count: i64,
}
