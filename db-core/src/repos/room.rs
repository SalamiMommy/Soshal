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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomReactionSummaryRow {
    pub message_id: String,
    pub emoji: String,
    pub count: i64,
    pub reacted: bool,
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
        crate::query::execute(
            &conn,
            "UPDATE group_room_reactions SET room_id = '' WHERE room_id = ?1",
            [room_id],
        )?;
        crate::query::execute(&conn, "DELETE FROM group_rooms WHERE id = ?1", [room_id])?;
        Ok(())
    }

    /// Toggle an emoji reaction on a post in a room; returns true when added, false when removed.
    pub fn toggle_reaction(
        &self,
        group_id: &str,
        room_id: &str,
        message_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let deleted = crate::query::execute(
            &conn,
            "DELETE FROM group_room_reactions
             WHERE message_id = ?1 AND LOWER(pubkey) = ?2 AND emoji = ?3",
            params![message_id, norm_pk.as_str(), emoji],
        )?;
        if deleted > 0 {
            return Ok(false);
        }
        crate::query::execute(
            &conn,
            "INSERT INTO group_room_reactions (group_id, room_id, message_id, pubkey, emoji, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                group_id,
                room_id,
                message_id,
                norm_pk.as_str(),
                emoji,
                soshal_common_core::format::now_secs()
            ],
        )?;
        Ok(true)
    }

    /// Emoji reaction counts for all posts in a room, each with whether `viewer_pubkey` reacted.
    pub fn reaction_summary(
        &self,
        group_id: &str,
        room_id: &str,
        viewer_pubkey: &str,
    ) -> Result<Vec<RoomReactionSummaryRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_viewer = viewer_pubkey.trim().to_ascii_lowercase();
        crate::query::query(
            &conn,
            "SELECT r.message_id, r.emoji, COUNT(*) AS cnt,
                    MAX(CASE WHEN LOWER(r.pubkey) = ?3 THEN 1 ELSE 0 END) AS reacted
             FROM group_room_reactions r
             WHERE r.group_id = ?1 AND r.room_id = ?2
             GROUP BY r.message_id, r.emoji
             ORDER BY cnt DESC, r.emoji ASC",
            [group_id, room_id, norm_viewer.as_str()],
            |row| {
                Ok(RoomReactionSummaryRow {
                    message_id: row.get(0)?,
                    emoji: row.get(1)?,
                    count: row.get(2)?,
                    reacted: row.get::<i64>(3)? != 0,
                })
            },
        )
    }
}
