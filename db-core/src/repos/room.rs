use libsql::params;
use serde::{Deserialize, Serialize};

use crate::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRoomRow {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub topic: String,
    pub emoji: String,
    pub color: String,
    pub position: i64,
    pub created_by: String,
    pub created_at: i64,
}

pub struct GroupRoomRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupRoomRepo<'a> {
    soshal_repo_new!();

    pub fn list(&self, group_id: &str) -> Result<Vec<GroupRoomRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, group_id, name, topic, emoji, color, position, created_by, created_at
             FROM group_rooms WHERE group_id = ?1 ORDER BY position ASC, created_at ASC",
            [group_id],
            |row| {
                Ok(GroupRoomRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                    topic: row.get(3)?,
                    emoji: row.get(4)?,
                    color: row.get(5)?,
                    position: row.get(6)?,
                    created_by: row.get(7)?,
                    created_at: row.get(8)?,
                })
            },
        )
    }

    pub fn get(&self, room_id: &str) -> Result<Option<GroupRoomRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, group_id, name, topic, emoji, color, position, created_by, created_at
             FROM group_rooms WHERE id = ?1",
            [room_id],
            |row| {
                Ok(GroupRoomRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                    topic: row.get(3)?,
                    emoji: row.get(4)?,
                    color: row.get(5)?,
                    position: row.get(6)?,
                    created_by: row.get(7)?,
                    created_at: row.get(8)?,
                })
            },
        )
    }

    pub fn upsert(&self, room: &GroupRoomRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_rooms (id, group_id, name, topic, emoji, color, position, created_by, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                room.id.as_str(),
                room.group_id.as_str(),
                room.name.as_str(),
                room.topic.as_str(),
                room.emoji.as_str(),
                room.color.as_str(),
                room.position,
                room.created_by.as_str(),
                room.created_at
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, room_id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM group_rooms WHERE id = ?1", [room_id])?;
        Ok(())
    }
}
