use crate::Database;
use libsql::params;

pub struct PollRepo<'a> {
    db: &'a Database,
}

impl<'a> PollRepo<'a> {
    soshal_repo_new!();

    pub fn upsert_poll(&self, p: &PollRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO polls (id, pubkey, question, options, expires_at, closed, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET question=excluded.question, options=excluded.options, expires_at=excluded.expires_at, closed=excluded.closed",
            params![
                p.id.as_str(),
                p.pubkey.as_str(),
                p.question.as_str(),
                p.options.as_str(),
                p.expires_at,
                p.closed,
                p.created_at
            ],
        )?;
        Ok(())
    }

    pub fn get_poll(&self, id: &str) -> Result<Option<PollRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT id, pubkey, question, options, expires_at, closed, created_at FROM polls WHERE id=?1",
            params![id],
            Self::map_poll,
        )
    }

    pub fn list_by_author(
        &self,
        pubkey: &str,
        limit: i64,
    ) -> Result<Vec<PollRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, question, options, expires_at, closed, created_at FROM polls WHERE pubkey=?1 ORDER BY created_at DESC LIMIT ?2",
            params![pubkey, limit],
            Self::map_poll,
        )
    }

    pub fn set_closed(&self, id: &str, closed: bool) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE polls SET closed=?2 WHERE id=?1",
            params![id, closed as i64],
        )?;
        Ok(())
    }

    pub fn vote(&self, v: &PollVoteRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO poll_votes (id, poll_id, option_id, voter_pubkey, voted_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(poll_id, voter_pubkey) DO UPDATE SET option_id=excluded.option_id, voted_at=excluded.voted_at",
            params![v.id.as_str(), v.poll_id.as_str(), v.option_id, v.voter_pubkey.as_str(), v.voted_at],
        )?;
        Ok(())
    }

    pub fn has_voted(&self, poll_id: &str, voter: &str) -> Result<bool, crate::error::DbError> {
        let conn = self.db.conn()?;
        Ok(crate::query::query_first(
            &conn,
            "SELECT 1 FROM poll_votes WHERE poll_id=?1 AND voter_pubkey=?2",
            params![poll_id, voter],
            |_| Ok(true),
        )?
        .is_some())
    }

    pub fn option_count(
        &self,
        poll_id: &str,
        option_id: i64,
    ) -> Result<i64, crate::error::DbError> {
        let conn = self.db.conn()?;
        let v = crate::query::query_first(
            &conn,
            "SELECT COUNT(*) FROM poll_votes WHERE poll_id=?1 AND option_id=?2",
            params![poll_id, option_id],
            |r| r.get::<i64>(0),
        )?;
        Ok(v.unwrap_or(0))
    }

    fn map_poll(r: &libsql::Row) -> libsql::Result<PollRow> {
        Ok(PollRow {
            id: r.get(0)?,
            pubkey: r.get(1)?,
            question: r.get(2)?,
            options: r.get(3)?,
            expires_at: r.get(4)?,
            closed: r.get(5)?,
            created_at: r.get(6)?,
        })
    }
}

pub struct PollRow {
    pub id: String,
    pub pubkey: String,
    pub question: String,
    pub options: String,
    pub expires_at: i64,
    pub closed: bool,
    pub created_at: i64,
}

pub struct PollVoteRow {
    pub id: String,
    pub poll_id: String,
    pub option_id: i64,
    pub voter_pubkey: String,
    pub voted_at: i64,
}
