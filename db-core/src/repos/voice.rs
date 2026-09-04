use libsql::params;
use serde::{Deserialize, Serialize};

use crate::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupVoiceChannelRow {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub position: i64,
    pub created_by: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupVoicePresenceRow {
    pub channel_id: String,
    pub pubkey: String,
    pub joined_at: i64,
}

pub struct GroupVoiceRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupVoiceRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn list_channels(
        &self,
        group_id: &str,
    ) -> Result<Vec<GroupVoiceChannelRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, group_id, name, position, created_by, created_at
             FROM group_voice_channels WHERE group_id = ?1 ORDER BY position ASC, created_at ASC",
            [group_id],
            |row| {
                Ok(GroupVoiceChannelRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                    position: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
    }

    pub fn upsert_channel(
        &self,
        channel: &GroupVoiceChannelRow,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_voice_channels (id, group_id, name, position, created_by, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                channel.id.as_str(),
                channel.group_id.as_str(),
                channel.name.as_str(),
                channel.position,
                channel.created_by.as_str(),
                channel.created_at
            ],
        )?;
        Ok(())
    }

    pub fn delete_channel(&self, channel_id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            tx.execute(
                "DELETE FROM group_voice_presence WHERE channel_id = ?1",
                params![channel_id],
            )
            .await?;
            tx.execute(
                "DELETE FROM group_voice_channels WHERE id = ?1",
                params![channel_id],
            )
            .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub fn presence(
        &self,
        channel_id: &str,
    ) -> Result<Vec<GroupVoicePresenceRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT channel_id, pubkey, joined_at
             FROM group_voice_presence WHERE channel_id = ?1 ORDER BY joined_at ASC",
            [channel_id],
            |row| {
                Ok(GroupVoicePresenceRow {
                    channel_id: row.get(0)?,
                    pubkey: row.get(1)?,
                    joined_at: row.get(2)?,
                })
            },
        )
    }

    pub fn join(&self, channel_id: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO group_voice_presence (channel_id, pubkey, joined_at)
             VALUES (?1,?2,?3)",
            params![channel_id, pubkey, soshal_common_core::format::now_secs()],
        )?;
        Ok(())
    }

    pub fn leave(&self, channel_id: &str, pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM group_voice_presence WHERE channel_id = ?1 AND pubkey = ?2",
            params![channel_id, pubkey],
        )?;
        Ok(())
    }
}
