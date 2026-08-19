use crate::Database;
use libsql::params;

pub struct GroupRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<GroupRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, name, about, picture, pubkey, created_at, updated_at, access_type, relay, sync_status, password_hash FROM groups WHERE id = ?1",
            params![id],
            Self::map_row,
        )
    }

    pub fn get_user_groups(&self, pubkey: &str) -> Result<Vec<GroupRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT g.id, g.name, g.about, g.picture, g.pubkey, g.created_at, g.updated_at, g.access_type, g.relay, g.sync_status, g.password_hash FROM groups g JOIN group_members gm ON g.id = gm.group_id WHERE gm.pubkey = ?1 ORDER BY g.updated_at DESC",
            params![pubkey],
            Self::map_row,
        )
    }

    pub fn upsert(&self, group: &GroupRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO groups (id, name, about, picture, pubkey, created_at, updated_at, access_type, relay, sync_status, password_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET name=excluded.name, about=excluded.about, picture=excluded.picture, updated_at=excluded.updated_at, access_type=excluded.access_type, relay=excluded.relay, sync_status=excluded.sync_status, password_hash=excluded.password_hash",
            params![
                group.id.as_str(),
                group.name.as_str(),
                group.about.as_deref(),
                group.picture.as_deref(),
                group.pubkey.as_str(),
                group.created_at,
                group.updated_at,
                group.access_type.as_str(),
                group.relay.as_deref(),
                group.sync_status.as_str(),
                group.password_hash.as_deref(),
            ],
        )?;
        Ok(())
    }

    pub fn add_member(
        &self,
        group_id: &str,
        pubkey: &str,
        role: &str,
        joined_at: i64,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_members (group_id, pubkey, role, joined_at) VALUES (?1,?2,?3,?4)",
            params![group_id, pubkey, role, joined_at],
        )?;
        Ok(())
    }

    pub fn remove_member(&self, group_id: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_members WHERE group_id = ?1 AND pubkey = ?2",
            params![group_id, pubkey],
        )?;
        Ok(())
    }

    pub fn get_members(
        &self,
        group_id: &str,
    ) -> Result<Vec<GroupMemberRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT group_id, pubkey, role, joined_at FROM group_members WHERE group_id = ?1 ORDER BY joined_at ASC",
            params![group_id],
            |row| {
                Ok(GroupMemberRow {
                    group_id: row.get(0)?,
                    pubkey: row.get(1)?,
                    role: row.get(2)?,
                    joined_at: row.get(3)?,
                })
            },
        )
    }

    /// Fetch the shared key for a group, or None when absent.
    pub fn get_shared_key(&self, group_id: &str) -> Result<Option<String>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT key_hex FROM group_shared_keys WHERE group_id = ?1",
            params![group_id],
            |row| row.get(0),
        )
    }

    /// Store the shared key for a group, replacing any previous value.
    pub fn set_shared_key(
        &self,
        group_id: &str,
        key_hex: &str,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_shared_keys (group_id, key_hex, updated_at) VALUES (?1,?2,?3)",
            params![group_id, key_hex, soshal_common_core::format::now_secs()],
        )?;
        Ok(())
    }

    pub fn member_count_many(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, i64>, crate::error::DbError> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let conn = self.db.conn()?;
        let json_ids = serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string());
        crate::query::query(
            &conn,
            "SELECT group_id, COUNT(*) AS c FROM group_members \
             WHERE group_id IN (SELECT value FROM json_each(?1)) GROUP BY group_id",
            params![json_ids],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map(|rows| rows.into_iter().collect())
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<GroupRow> {
        Ok(GroupRow {
            id: row.get(0)?,
            name: row.get(1)?,
            about: row.get(2)?,
            picture: row.get(3)?,
            pubkey: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            access_type: row.get(7)?,
            relay: row.get(8)?,
            sync_status: row.get(9)?,
            password_hash: row.get(10)?,
        })
    }
}

pub struct GroupRow {
    pub id: String,
    pub name: String,
    pub about: Option<String>,
    pub picture: Option<String>,
    pub pubkey: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub access_type: String,
    pub relay: Option<String>,
    pub sync_status: String,
    pub password_hash: Option<String>,
}

pub struct GroupMemberRow {
    pub group_id: String,
    pub pubkey: String,
    pub role: String,
    pub joined_at: i64,
}
