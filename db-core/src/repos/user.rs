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

    pub fn upsert(&self, user: &UserRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO users (pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(pubkey) DO UPDATE SET name=excluded.name, display_name=excluded.display_name, about=excluded.about, picture=excluded.picture, banner=excluded.banner, nip05=excluded.nip05, lud16=excluded.lud16, updated_at=excluded.updated_at, metadata_json=excluded.metadata_json, contact_pubkeys=excluded.contact_pubkeys, relay_list=excluded.relay_list",
            params![
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
            ],
        )?;
        Ok(())
    }

    pub fn upsert_batch(&self, users: &[UserRow]) -> Result<(), crate::error::DbError> {
        if users.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let sql = "INSERT INTO users (pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(pubkey) DO UPDATE SET name=excluded.name, display_name=excluded.display_name, about=excluded.about, picture=excluded.picture, banner=excluded.banner, nip05=excluded.nip05, lud16=excluded.lud16, updated_at=excluded.updated_at, metadata_json=excluded.metadata_json, contact_pubkeys=excluded.contact_pubkeys, relay_list=excluded.relay_list";
            for user in users {
                tx.execute(
                    sql,
                    params![
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
                    ],
                )
                .await?;
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
        let conn = self.db.conn()?;
        // Escape LIKE wildcards so user input matches literally.
        let escaped: String = query
            .chars()
            .take(64)
            .flat_map(|c| match c {
                '%' | '_' | '\\' => vec!['\\', c],
                c => vec![c],
            })
            .collect();
        let pattern = format!("%{}%", escaped);
        let limit = crate::repos::clamp_limit(limit);
        crate::query::query(
            &conn,
            "SELECT pubkey, npub, name, display_name, about, picture, banner, nip05, lud16, created_at, updated_at, metadata_json, contact_pubkeys, relay_list FROM users WHERE name LIKE ?1 ESCAPE '\\' OR display_name LIKE ?1 ESCAPE '\\' OR npub LIKE ?1 ESCAPE '\\' LIMIT ?2",
            params![pattern, limit],
            Self::map_row,
        )
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
