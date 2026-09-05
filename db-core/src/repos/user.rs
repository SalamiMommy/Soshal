use crate::Database;
use libsql::params;

const USER_UPSERT_SQL: &str = "\
INSERT INTO users (pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list, follower_count) \
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) \
ON CONFLICT(pubkey) DO UPDATE SET \
  name=excluded.name, display_name=excluded.display_name, about=excluded.about, \
  picture=excluded.picture, banner=excluded.banner, nip05=excluded.nip05, lud16=excluded.lud16, \
  updated_at=excluded.updated_at, metadata_json=excluded.metadata_json, contact_pubkeys=excluded.contact_pubkeys, \
  relay_list=excluded.relay_list";

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
            "SELECT pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list, follower_count FROM users WHERE pubkey = ?1",
            params![pubkey],
            Self::map_row,
        )
    }

    /// Which of the given pubkeys have a stored user row. One indexed query
    /// instead of N `get_by_pubkey` calls (mDNS peer drain runs this per
    /// drain tick).
    pub fn existing_pubkeys(
        &self,
        pubkeys: &[String],
    ) -> Result<std::collections::HashSet<String>, crate::error::DbError> {
        if pubkeys.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        let conn = self.db.conn()?;
        let json = serde_json::to_string(pubkeys).unwrap_or_else(|_| "[]".to_string());
        let found = crate::query::query(
            &conn,
            "SELECT pubkey FROM users WHERE pubkey IN (SELECT value FROM json_each(?1))",
            params![json.as_str()],
            |row| row.get::<String>(0),
        )?;
        Ok(found.into_iter().collect())
    }

    pub async fn get_by_pubkey_in(
        &self,
        tx: &libsql::Transaction,
        pubkey: &str,
    ) -> Result<Option<UserRow>, crate::error::DbError> {
        let stmt = tx
            .prepare("SELECT pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list, follower_count FROM users WHERE pubkey = ?1")
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
            USER_UPSERT_SQL,
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
                row.follower_count,
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
            let stmt = tx.prepare(USER_UPSERT_SQL).await?;
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
                    user.follower_count,
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

    /// Adjust the materialized `follower_count` for the given pubkeys. Called
    /// when a contact list changes: `+1` for each pubkey newly followed by an
    /// author, `-1` for each unfollowed. The count is maintained incrementally
    /// instead of being recomputed on every upsert (which was O(users) per
    /// contact-list event and stale for everyone except the event author).
    pub async fn bump_follower_count_in(
        &self,
        tx: &libsql::Transaction,
        pubkey: &str,
        delta: i64,
    ) -> Result<(), crate::error::DbError> {
        tx.execute(
            "UPDATE users SET follower_count = MAX(0, follower_count + ?2) WHERE pubkey = ?1",
            params![pubkey, delta],
        )
        .await?;
        Ok(())
    }

    pub fn bump_follower_count(
        &self,
        pubkey: &str,
        delta: i64,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            self.bump_follower_count_in(&tx, pubkey, delta).await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Re-materialize follower counts for every stored contact list in one
    /// indexed pass. Self-referential entries are excluded, and unknown
    /// pubkeys (followed users with no profile row yet) cause an empty
    /// `INSERT OR IGNORE` sink first so their count can be written.
    pub fn recompute_all_follower_counts(&self) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        // Reset first so removals from previous lists don't leak into the
        // recount.
        crate::query::execute(&conn, "UPDATE users SET follower_count = 0", ())?;
        let rows = crate::query::query(
            &conn,
            "SELECT pubkey, contact_pubkeys FROM users",
            (),
            |row| Ok((row.get::<String>(0)?, row.get::<String>(1)?)),
        )?;
        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for (author, list) in rows {
            let entries: Vec<String> = if list.trim().starts_with('[') {
                serde_json::from_str::<Vec<String>>(&list).unwrap_or_default()
            } else {
                list.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            };
            for pk in entries {
                if pk != author {
                    *counts.entry(pk).or_insert(0) += 1;
                }
            }
        }
        if counts.is_empty() {
            return Ok(());
        }
        crate::query::with_tx(&conn, |tx| async move {
            let mut stmt = tx
                .prepare("INSERT OR IGNORE INTO users (pubkey, npub) VALUES (?1, '')")
                .await?;
            for pk in counts.keys() {
                stmt.run(params![pk.as_str()]).await?;
                stmt.reset();
            }
            stmt = tx
                .prepare("UPDATE users SET follower_count = ?2 WHERE pubkey = ?1")
                .await?;
            for (pk, n) in &counts {
                stmt.run(params![pk.as_str(), *n]).await?;
                stmt.reset();
            }
            tx.commit().await?;
            Ok(())
        })
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
             u.relay_list, u.follower_count \
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
            follower_count: row.get(14)?,
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
    pub follower_count: i64,
}
