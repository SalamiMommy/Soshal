use crate::Database;
use libsql::params;

pub struct LinkPreviewRepo<'a> {
    db: &'a Database,
}

impl<'a> LinkPreviewRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, p: &LinkPreviewRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO link_previews (url, domain, title, description, image, favicon, cached_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(url) DO UPDATE SET title=excluded.title, description=excluded.description, image=excluded.image, favicon=excluded.favicon, cached_at=excluded.cached_at",
            params![p.url.as_str(), p.domain.as_str(), p.title.as_str(), p.description.as_str(), p.image.as_deref(), p.favicon.as_deref(), p.cached_at],
        )?;
        Ok(())
    }

    pub fn get(&self, url: &str) -> Result<Option<LinkPreviewRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT url, domain, title, description, image, favicon, cached_at FROM link_previews WHERE url=?1",
            params![url],
            Self::map_row,
        )
    }

    pub fn list_recent(&self, limit: i64) -> Result<Vec<LinkPreviewRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT url, domain, title, description, image, favicon, cached_at FROM link_previews ORDER BY cached_at DESC LIMIT ?1",
            params![limit],
            Self::map_row,
        )
    }

    pub fn delete(&self, url: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM link_previews WHERE url=?1",
            params![url],
        )?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<LinkPreviewRow> {
        Ok(LinkPreviewRow {
            url: r.get(0)?,
            domain: r.get(1)?,
            title: r.get(2)?,
            description: r.get(3)?,
            image: r.get(4)?,
            favicon: r.get(5)?,
            cached_at: r.get(6)?,
        })
    }
}

pub struct LinkPreviewRow {
    pub url: String,
    pub domain: String,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
    pub favicon: Option<String>,
    pub cached_at: i64,
}
