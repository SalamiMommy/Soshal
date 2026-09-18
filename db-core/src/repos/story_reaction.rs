use crate::Database;
use libsql::params;

pub struct StoryReactionRepo<'a> {
    db: &'a Database,
}

impl<'a> StoryReactionRepo<'a> {
    soshal_repo_new!();

    pub fn react(&self, r: &StoryReactionRow) -> Result<(), crate::error::DbError> {
        let norm_pk = r.pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO story_reactions (story_id, pubkey, emoji, created_at) VALUES (?1,?2,?3,?4) ON CONFLICT(story_id,pubkey,emoji) DO NOTHING",
            params![r.story_id.trim(), norm_pk.as_str(), r.emoji.trim(), r.created_at],
        )?;
        Ok(())
    }

    pub fn unreact(
        &self,
        story_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<(), crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM story_reactions WHERE story_id=?1 AND LOWER(pubkey)=LOWER(?2) AND emoji=?3",
            params![story_id.trim(), norm_pk.as_str(), emoji.trim()],
        )?;
        Ok(())
    }

    pub fn list_by_story(
        &self,
        story_id: &str,
        limit: i64,
    ) -> Result<Vec<StoryReactionRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT story_id, pubkey, emoji, created_at FROM story_reactions WHERE story_id=?1 ORDER BY created_at DESC LIMIT ?2",
            params![story_id.trim(), limit],
            Self::map_row,
        )
    }

    pub fn reacted_with(
        &self,
        story_id: &str,
        pubkey: &str,
        emoji: &str,
    ) -> Result<bool, crate::error::DbError> {
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM story_reactions WHERE story_id=?1 AND LOWER(pubkey)=LOWER(?2) AND emoji=?3",
            params![story_id.trim(), norm_pk.as_str(), emoji.trim()],
            |_| Ok(true),
        )?
        .is_some())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<StoryReactionRow> {
        Ok(StoryReactionRow {
            story_id: r.get(0)?,
            pubkey: r.get(1)?,
            emoji: r.get(2)?,
            created_at: r.get(3)?,
        })
    }
}

pub struct StoryReactionRow {
    pub story_id: String,
    pub pubkey: String,
    pub emoji: String,
    pub created_at: i64,
}
