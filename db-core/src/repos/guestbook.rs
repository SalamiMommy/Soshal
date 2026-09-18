use crate::Database;
use libsql::params;

pub struct GuestbookRepo<'a> {
    db: &'a Database,
}

impl<'a> GuestbookRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, e: &GuestbookEntryRow) -> Result<(), crate::error::DbError> {
        let norm_profile = e.profile_pubkey.trim().to_ascii_lowercase();
        let norm_sender = e.sender_pubkey.trim().to_ascii_lowercase();
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO guestbook_entries (id, profile_pubkey, sender_pubkey, sender_name, sender_avatar, content, created_at, signature, approved) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO NOTHING",
            params![
                e.id.trim(),
                norm_profile.as_str(),
                norm_sender.as_str(),
                e.sender_name.as_deref(),
                e.sender_avatar.as_deref(),
                e.content.as_str(),
                e.created_at,
                e.signature.as_deref(),
                e.approved as i64
            ],
        )?;
        Ok(())
    }

    pub fn list_by_profile(
        &self,
        profile_pubkey: &str,
        limit: i64,
        only_approved: bool,
    ) -> Result<Vec<GuestbookEntryRow>, crate::error::DbError> {
        let norm_profile = profile_pubkey.trim().to_ascii_lowercase();
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        let sql = if only_approved {
            "SELECT id, profile_pubkey, sender_pubkey, sender_name, sender_avatar, content, created_at, signature, approved FROM guestbook_entries WHERE LOWER(profile_pubkey)=LOWER(?1) AND approved=1 ORDER BY created_at DESC LIMIT ?2"
        } else {
            "SELECT id, profile_pubkey, sender_pubkey, sender_name, sender_avatar, content, created_at, signature, approved FROM guestbook_entries WHERE LOWER(profile_pubkey)=LOWER(?1) ORDER BY created_at DESC LIMIT ?2"
        };
        crate::query::query(
            &conn,
            sql,
            params![norm_profile.as_str(), limit],
            Self::map_row,
        )
    }

    pub fn set_approved(&self, id: &str, approved: bool) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE guestbook_entries SET approved=?2 WHERE id=?1",
            params![id.trim(), approved as i64],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM guestbook_entries WHERE id=?1",
            params![id.trim()],
        )?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<GuestbookEntryRow> {
        Ok(GuestbookEntryRow {
            id: r.get(0)?,
            profile_pubkey: r.get(1)?,
            sender_pubkey: r.get(2)?,
            sender_name: r.get(3)?,
            sender_avatar: r.get(4)?,
            content: r.get(5)?,
            created_at: r.get(6)?,
            signature: r.get(7)?,
            approved: r.get::<i64>(8)? != 0,
        })
    }
}

pub struct GuestbookEntryRow {
    pub id: String,
    pub profile_pubkey: String,
    pub sender_pubkey: String,
    pub sender_name: Option<String>,
    pub sender_avatar: Option<String>,
    pub content: String,
    pub created_at: i64,
    pub signature: Option<String>,
    pub approved: bool,
}
