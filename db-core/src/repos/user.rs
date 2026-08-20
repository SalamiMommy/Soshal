use crate::Database;
use libsql::params;

pub struct UserRepo<'a> {
    db: &'a Database,
}

impl<'a> UserRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_pubkey(&self, pubkey: &str) -> Result<Option<UserRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list FROM users WHERE pubkey = ?1",
            params![pubkey],
            Self::map_row,
        )
    }

    pub async fn get_by_pubkey_in(
        &self,
        tx: &libsql::Transaction,
        pubkey: &str,
    ) -> Result<Option<UserRow>, crate::error::DbError> {
        let stmt = tx
            .prepare("SELECT pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list FROM users WHERE pubkey = ?1")
            .await?;
        let mut rows = stmt.query(params![pubkey]).await?;
        match rows.next().await? {
            Some(row) => Ok(Some(Self::map_row(&row)?)),
            None => Ok(None),
        }
    }

    pub fn upsert(&self, user: &UserRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.upsert_in(&tx, user).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn upsert_in(
        &self,
        tx: &libsql::Transaction,
        row: &UserRow,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "INSERT INTO users (pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list, follower_count) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14, CASE WHEN json_valid(?13) THEN json_array_length(?13) ELSE 0 END) ON CONFLICT(pubkey) DO UPDATE SET name=excluded.name, display_name=excluded.display_name, about=excluded.about, picture=excluded.picture, banner=excluded.banner, nip05=excluded.nip05, lud16=excluded.lud16, updated_at=excluded.updated_at, metadata_json=excluded.metadata_json, contact_pubkeys=excluded.contact_pubkeys, relay_list=excluded.relay_list, follower_count=CASE WHEN json_valid(excluded.contact_pubkeys) THEN json_array_length(excluded.contact_pubkeys) ELSE 0 END",
            params![
                row.pubkey.as_str(),
                row.npub.as_str(),
                row.name.as_deref(),
                row.display_name.as_deref(),
                row.about.as_deref(),
                row.picture.as_deref(),
                row.banner.as_deref(),
                row.nip05.as_deref(),
                row.lud16.as_deref(),
                row.created_at,
                row.updated_at,
                row.metadata_json.as_deref(),
                row.contact_pubkeys.as_str(),
                row.relay_list.as_str(),
            ],
        )
        .await?;
        Ok(())
    }

    pub fn upsert_batch(&self, users: &[UserRow]) -> Result<(), crate::error::DbError> {
        if users.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let sql = "INSERT INTO users (pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list, follower_count) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14, CASE WHEN json_valid(?13) THEN json_array_length(?13) ELSE 0 END) ON CONFLICT(pubkey) DO UPDATE SET name=excluded.name, display_name=excluded.display_name, about=excluded.about, picture=excluded.picture, banner=excluded.banner, nip05=excluded.nip05, lud16=excluded.lud16, updated_at=excluded.updated_at, metadata_json=excluded.metadata_json, contact_pubkeys=excluded.contact_pubkeys, relay_list=excluded.relay_list, follower_count=CASE WHEN json_valid(excluded.contact_pubkeys) THEN json_array_length(excluded.contact_pubkeys) ELSE 0 END";
            let stmt = tx.prepare(sql).await?;
            for user in users {
                stmt.run(params![
                    user.pubkey.as_str(),
                    user.npub.as_str(),
                    user.name.as_deref(),
                    user.display_name.as_deref(),
                    user.about.as_deref(),
                    user.picture.as_deref(),
                    user.banner.as_deref(),
                    user.nip05.as_deref(),
                    user.lud16.as_deref(),
                    user.created_at,
                    user.updated_at,
                    user.metadata_json.as_deref(),
                    user.contact_pubkeys.as_str(),
                    user.relay_list.as_str(),
                ])
                .await?;
                stmt.reset();
            }
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn ensure_exists(&self, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')",
            params![pubkey],
        )?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<UserRow>, crate::error::DbError> {
        let fts = Self::fts_prefix_query(query);
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT u.pubkey, u.npub, u.name, u.display_name, u.about, u.picture, u.banner, \
             u.nip05, u.lud16, u.created_at, u.updated_at, u.metadata_json, u.contact_pubkeys, \
             u.relay_list \
             FROM users_fts f JOIN users u ON u.rowid = f.rowid \
             WHERE users_fts MATCH ?1 ORDER BY rank LIMIT ?2",
            params![fts, limit],
            Self::map_row,
        )
    }

    /// FTS5 MATCH expression with per-term prefix matching. Only
    /// alphanumerics survive, so user input can never smuggle FTS5
    /// operators (`*`, `OR`, quotes, parens) into the parser. Terms are
    /// split on any non-alphanumeric character (mirrors unicode61
    /// tokenization, where `_`/`-` separate tokens) and AND-joined.
    fn fts_prefix_query(query: &str) -> String {
        let mut out = String::with_capacity(query.len().saturating_add(16));
        let mut first = true;
        let mut term = String::with_capacity(16);
        for c in query.chars() {
            if c.is_alphanumeric() {
                term.push(c.to_ascii_lowercase());
                continue;
            }
            if !term.is_empty() {
                if !first {
                    out.push(' ');
                } else {
                    first = false;
                }
                out.push('"');
                out.push_str(&term);
                out.push_str("\"*");
                term.clear();
            }
        }
        if !term.is_empty() {
            if !first {
                out.push(' ');
            }
            out.push('"');
            out.push_str(&term);
            out.push_str("\"*");
        }
        out
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<UserRow> {
        Ok(UserRow {
            pubkey: row.get(0)?,
            npub: row.get(1)?,
            name: row.get(2)?,
            display_name: row.get(3)?,
            about: row.get(4)?,
            picture: row.get(5)?,
            banner: row.get(6)?,
            nip05: row.get(7)?,
            lud16: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            metadata_json: row.get(11)?,
            contact_pubkeys: row.get(12)?,
            relay_list: row.get(13)?,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserRow {
    pub pubkey: String,
    pub npub: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub nip05: Option<String>,
    pub lud16: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub metadata_json: Option<String>,
    pub contact_pubkeys: String,
    pub relay_list: String,
}
