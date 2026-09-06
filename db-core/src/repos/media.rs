use crate::Database;
use libsql::params;

pub struct MediaRepo<'a> {
    db: &'a Database,
}

impl<'a> MediaRepo<'a> {
    soshal_repo_new!();

    pub fn get_by_id(&self, id: &str) -> Result<Option<MediaRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, pubkey, url, file_hash, file_size, mime_type, created_at, blob_hash FROM media_blobs WHERE id = ?1",
            params![id],
            Self::map_row,
        )
    }

    pub fn get_user_media(
        &self,
        pubkey: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MediaRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let offset = offset.max(0);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, url, file_hash, file_size, mime_type, created_at, blob_hash FROM media_blobs WHERE pubkey = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            params![pubkey, limit, offset],
            Self::map_row,
        )
    }

    pub fn upsert(&self, row: &MediaRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO media_blobs (id, pubkey, url, file_hash, file_size, mime_type, created_at, blob_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET url=excluded.url, file_hash=excluded.file_hash, file_size=excluded.file_size, mime_type=excluded.mime_type, blob_hash=excluded.blob_hash",
            params![
                row.id.as_str(),
                row.pubkey.as_str(),
                row.url.as_str(),
                row.file_hash.as_deref(),
                row.file_size,
                row.mime_type.as_deref(),
                row.created_at,
                row.blob_hash.as_deref(),
            ],
        )?;
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<MediaRow> {
        Ok(MediaRow {
            id: row.get(0)?,
            pubkey: row.get(1)?,
            url: row.get(2)?,
            file_hash: row.get(3)?,
            file_size: row.get(4)?,
            mime_type: row.get(5)?,
            created_at: row.get(6)?,
            blob_hash: row.get(7)?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MediaRow {
    pub id: String,
    pub pubkey: String,
    pub url: String,
    pub file_hash: Option<String>,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub created_at: i64,
    pub blob_hash: Option<String>,
}
