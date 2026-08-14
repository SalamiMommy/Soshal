use crate::Database;
use libsql::params;

pub struct SearchIndexRepo<'a> {
    db: &'a Database,
}

impl<'a> SearchIndexRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, row: &SearchIndexRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO posts_fts (id, pubkey, content) VALUES (?1, ?2, ?3)",
            params![row.id.as_str(), row.pubkey.as_str(), row.content.as_str()],
        )?;
        Ok(())
    }

    pub fn search(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SearchIndexRow>, crate::error::DbError> {
        let (limit, offset) = crate::repos::clamp_page(limit, offset);
        let conn = self.db.conn()?;
        let fts_query = build_fts_query(query);
        crate::query::query(
            &conn,
            "SELECT p.id, p.pubkey, p.content, p.kind, p.created_at FROM posts_fts f JOIN posts p ON f.rowid = p.rowid WHERE p.is_deleted = 0 AND posts_fts MATCH ?1 ORDER BY rank LIMIT ?2 OFFSET ?3",
            params![fts_query, limit, offset],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM posts_fts WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<SearchIndexRow> {
        Ok(SearchIndexRow {
            id: row.get(0)?,
            pubkey: row.get(1)?,
            content: row.get(2)?,
            kind: row.get(3)?,
            created_at: row.get(4)?,
        })
    }
}

pub struct SearchIndexRow {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub kind: i64,
    pub created_at: i64,
}

/// Builds an FTS5 MATCH expression from a raw user query. Every term is
/// reduced to alphanumerics only, so FTS operators (`*`, `-`, `NEAR`,
/// parentheses, `NOT`) cannot reach the FTS5 parser as operators — this repo
/// stays safe even if a future caller forgets to pre-sanitize.
#[doc(hidden)]
pub fn build_fts_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len().saturating_add(16));
    let mut first = true;
    for w in query
        .split_whitespace()
        .take(soshal_content_core::fts5::MAX_FTS5_TERMS)
    {
        let mut cleaned =
            String::with_capacity(w.len().min(soshal_content_core::fts5::MAX_FTS5_TERM_LEN));
        for c in w
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(soshal_content_core::fts5::MAX_FTS5_TERM_LEN)
        {
            cleaned.push(c);
        }
        if cleaned.is_empty() {
            continue;
        }
        if !first {
            out.push_str(" OR ");
        } else {
            first = false;
        }
        out.push('"');
        out.push_str(&cleaned);
        out.push('"');
    }
    out
}
