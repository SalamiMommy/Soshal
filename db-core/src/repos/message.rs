use crate::Database;
use libsql::params;

pub struct MessageRepo<'a> {
    db: &'a Database,
}

impl<'a> MessageRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<MessageRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, conversation_id, pubkey, content, created_at, tags_json, reply_to, sync_status, is_deleted FROM messages WHERE id = ?1",
            params![id],
            Self::map_row,
        )
    }

    pub fn get_conversation(
        &self,
        conversation_id: &str,
        limit: i64,
        before: Option<(i64, String)>,
    ) -> Result<Vec<MessageRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        match before {
            Some((ts, id)) => crate::query::query(
                &conn,
                "SELECT id, conversation_id, pubkey, content, created_at, tags_json, reply_to, sync_status, is_deleted FROM messages WHERE conversation_id = ?1 AND is_deleted = 0 AND (created_at < ?2 OR (created_at = ?2 AND id < ?3)) ORDER BY created_at DESC, id DESC LIMIT ?4",
                params![conversation_id, ts, id, limit],
                Self::map_row,
            ),
            None => crate::query::query(
                &conn,
                "SELECT id, conversation_id, pubkey, content, created_at, tags_json, reply_to, sync_status, is_deleted FROM messages WHERE conversation_id = ?1 AND is_deleted = 0 ORDER BY created_at DESC, id DESC LIMIT ?2",
                params![conversation_id, limit],
                Self::map_row,
            ),
        }
    }

    pub fn upsert(&self, msg: &MessageRow) -> Result<(), crate::error::DbError> {
        if crate::repos::limits::row_too_big(&msg.content, &msg.tags_json) {
            return Err(crate::error::DbError::Oversized(format!(
                "message {} exceeds relay size caps ({} / {} bytes)",
                msg.id,
                crate::repos::limits::MAX_CONTENT_BYTES,
                crate::repos::limits::MAX_BATCH_BYTES
            )));
        }
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO messages (id, conversation_id, pubkey, content, created_at, tags_json, reply_to, sync_status, is_deleted) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET content=excluded.content, tags_json=excluded.tags_json, sync_status=excluded.sync_status, is_deleted=excluded.is_deleted",
            params![
                msg.id.as_str(),
                msg.conversation_id.as_str(),
                msg.pubkey.as_str(),
                msg.content.as_str(),
                msg.created_at,
                msg.tags_json.as_str(),
                msg.reply_to.as_deref(),
                msg.sync_status.as_str(),
                msg.is_deleted,
            ],
        )?;
        self.touch_conversation(&conn, &msg.conversation_id, msg.created_at)?;
        Ok(())
    }

    /// Keep the inbox `conversations` table fresh: bump the conversation's
    /// last-message timestamp. Only 'conv:'-prefixed ids are listed by the
    /// inbox query; group-DM chats are excluded here.
    fn touch_conversation(
        &self,
        conn: &libsql::Connection,
        conversation_id: &str,
        created_at: i64,
    ) -> Result<(), crate::error::DbError> {
        if !conversation_id.starts_with("conv:") {
            return Ok(());
        }
        crate::query::execute(
            conn,
            "INSERT INTO conversations (conversation_id, last_message_at) VALUES (?1, ?2) \
             ON CONFLICT(conversation_id) DO UPDATE SET \
             last_message_at = MAX(last_message_at, excluded.last_message_at)",
            params![conversation_id, created_at],
        )?;
        Ok(())
    }

    pub fn upsert_batch(&self, messages: &[MessageRow]) -> Result<(), crate::error::DbError> {
        if messages.is_empty() {
            return Ok(());
        }
        let conn = self.db.conn()?;
        crate::query::with_tx(&conn, |tx| async move {
            let sql = "INSERT INTO messages (id, conversation_id, pubkey, content, created_at, tags_json, reply_to, sync_status, is_deleted) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET content=excluded.content, tags_json=excluded.tags_json, sync_status=excluded.sync_status, is_deleted=excluded.is_deleted";
            // libsql quirk: re-executing a prepared UPSERT statement inside a
            // transaction silently no-ops on conflict; execute per row instead.
            let mut convs: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
            for msg in messages {
                if crate::repos::limits::row_too_big(&msg.content, &msg.tags_json) {
                    continue; // relay content too large: skip, never store
                }
                tx.execute(
                    sql,
                    params![
                        msg.id.as_str(),
                        msg.conversation_id.as_str(),
                        msg.pubkey.as_str(),
                        msg.content.as_str(),
                        msg.created_at,
                        msg.tags_json.as_str(),
                        msg.reply_to.as_deref(),
                        msg.sync_status.as_str(),
                        msg.is_deleted,
                    ],
                )
                .await?;
                if msg.conversation_id.starts_with("conv:") {
                    convs
                        .entry(msg.conversation_id.as_str())
                        .and_modify(|ts| *ts = (*ts).max(msg.created_at))
                        .or_insert(msg.created_at);
                }
            }
            for (conv_id, at) in convs {
                tx.execute(
                    "INSERT INTO conversations (conversation_id, last_message_at) VALUES (?1, ?2) \
                     ON CONFLICT(conversation_id) DO UPDATE SET \
                     last_message_at = MAX(last_message_at, excluded.last_message_at)",
                    params![conv_id, at],
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    fn map_row(row: &libsql::Row) -> libsql::Result<MessageRow> {
        Ok(MessageRow {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            pubkey: row.get(2)?,
            content: row.get(3)?,
            created_at: row.get(4)?,
            tags_json: row.get(5)?,
            reply_to: row.get(6)?,
            sync_status: row.get(7)?,
            is_deleted: row.get(8)?,
        })
    }
}

pub struct MessageRow {
    pub id: String,
    pub conversation_id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: i64,
    pub tags_json: String,
    pub reply_to: Option<String>,
    pub sync_status: String,
    pub is_deleted: bool,
}
