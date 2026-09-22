use crate::Database;
use libsql::params;

pub struct MusicloudRepo<'a> {
    db: &'a Database,
}

impl<'a> MusicloudRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, m: &MusicloudRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = m.pubkey.trim().to_ascii_lowercase();
        crate::query::execute(
            &conn,
            "INSERT INTO musiclouds (id, pubkey, audio_url, title, duration, text_overlay, thumbnail, likes, liked, bookmarked, audience, blob_hash, media_size, hashtags, d, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(id) DO UPDATE SET pubkey=excluded.pubkey, audio_url=excluded.audio_url, title=excluded.title, duration=excluded.duration, text_overlay=excluded.text_overlay, thumbnail=excluded.thumbnail, audience=excluded.audience, blob_hash=excluded.blob_hash, media_size=excluded.media_size, hashtags=excluded.hashtags, d=excluded.d, created_at=excluded.created_at",
            params![
                m.id.as_str(),
                norm_pk.as_str(),
                m.audio_url.as_str(),
                m.title.as_deref(),
                m.duration,
                m.text_overlay.as_deref(),
                m.thumbnail.as_deref(),
                m.likes,
                m.liked as i64,
                m.bookmarked as i64,
                m.audience.as_str(),
                m.blob_hash.as_str(),
                m.media_size,
                m.hashtags.as_str(),
                m.d.as_str(),
                m.created_at
            ],
        )?;
        Ok(())
    }

    pub fn list(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MusicloudRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, pubkey, audio_url, title, duration, text_overlay, thumbnail, likes, liked, bookmarked, audience, blob_hash, media_size, hashtags, d, created_at FROM musiclouds ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            params![limit, offset],
            Self::map_row,
        )
    }

    pub fn list_by_author(
        &self,
        pubkey: &str,
        limit: i64,
    ) -> Result<Vec<MusicloudRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        let norm_pk = pubkey.trim().to_ascii_lowercase();
        crate::query::query(
            &conn,
            "SELECT id, pubkey, audio_url, title, duration, text_overlay, thumbnail, likes, liked, bookmarked, audience, blob_hash, media_size, hashtags, d, created_at FROM musiclouds WHERE LOWER(pubkey)=?1 ORDER BY created_at DESC LIMIT ?2",
            params![norm_pk.as_str(), limit],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM musiclouds WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn set_like(&self, id: &str, liked: bool) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE musiclouds SET liked=?1, likes=CASE WHEN ?1=1 AND liked=0 THEN likes+1 WHEN ?1=0 AND liked=1 THEN MAX(likes-1, 0) ELSE likes END WHERE id=?2",
            params![liked as i64, id],
        )?;
        Ok(())
    }

    pub fn set_bookmark(&self, id: &str, bookmarked: bool) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE musiclouds SET bookmarked=?1 WHERE id=?2",
            params![bookmarked as i64, id],
        )?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<MusicloudRow> {
        Ok(MusicloudRow {
            id: r.get(0)?,
            pubkey: r.get(1)?,
            audio_url: r.get(2)?,
            title: r.get(3)?,
            duration: r.get(4)?,
            text_overlay: r.get(5)?,
            thumbnail: r.get(6)?,
            likes: r.get(7)?,
            liked: r.get(8)?,
            bookmarked: r.get(9)?,
            audience: r.get(10)?,
            blob_hash: r.get(11)?,
            media_size: r.get(12)?,
            hashtags: r.get(13)?,
            d: r.get(14)?,
            created_at: r.get(15)?,
        })
    }
}

pub struct MusicloudRow {
    pub id: String,
    pub pubkey: String,
    pub audio_url: String,
    pub title: Option<String>,
    pub duration: Option<i64>,
    pub text_overlay: Option<String>,
    pub thumbnail: Option<String>,
    pub likes: i64,
    pub liked: bool,
    pub bookmarked: bool,
    pub audience: String,
    pub blob_hash: String,
    pub media_size: i64,
    pub hashtags: String,
    pub d: String,
    pub created_at: i64,
}

pub struct MusicloudCommentRepo<'a> {
    db: &'a Database,
}

impl<'a> MusicloudCommentRepo<'a> {
    soshal_repo_new!();

    pub fn insert(&self, c: &MusicloudCommentRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let norm_pk = c.pubkey.trim().to_ascii_lowercase();
        crate::query::execute(
            &conn,
            "INSERT INTO musicloud_comments (id, track_id, pubkey, content, created_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO NOTHING",
            params![c.id.as_str(), c.track_id.as_str(), norm_pk.as_str(), c.content.as_str(), c.created_at],
        )?;
        Ok(())
    }

    pub fn list_by_track(
        &self,
        track_id: &str,
        limit: i64,
    ) -> Result<Vec<MusicloudCommentRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, track_id, pubkey, content, created_at FROM musicloud_comments WHERE track_id=?1 ORDER BY created_at ASC LIMIT ?2",
            params![track_id, limit],
            Self::map_row,
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<MusicloudCommentRow> {
        Ok(MusicloudCommentRow {
            id: r.get(0)?,
            track_id: r.get(1)?,
            pubkey: r.get(2)?,
            content: r.get(3)?,
            created_at: r.get(4)?,
        })
    }
}

pub struct MusicloudCommentRow {
    pub id: String,
    pub track_id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn insert_row(db: &Database, id: &str) {
        let repo = MusicloudRepo::new(db);
        let row = MusicloudRow {
            id: id.into(),
            pubkey: "pk".into(),
            audio_url: "https://example.com/a.mp3".into(),
            title: None,
            duration: None,
            text_overlay: None,
            thumbnail: None,
            likes: 0,
            liked: false,
            bookmarked: false,
            audience: String::new(),
            blob_hash: String::new(),
            media_size: 0,
            hashtags: "[]".into(),
            d: String::new(),
            created_at: 1000,
        };
        repo.upsert(&row).unwrap();
    }

    fn likes_of(db: &Database, id: &str) -> i64 {
        let conn = db.conn().unwrap();
        crate::query::query_first(
            &conn,
            "SELECT likes FROM musiclouds WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn set_like_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();
        insert_row(&db, "m1");
        let repo = MusicloudRepo::new(&db);

        repo.set_like("m1", true).unwrap();
        assert_eq!(likes_of(&db, "m1"), 1);
        repo.set_like("m1", true).unwrap();
        assert_eq!(likes_of(&db, "m1"), 1);

        repo.set_like("m1", false).unwrap();
        assert_eq!(likes_of(&db, "m1"), 0);
        repo.set_like("m1", false).unwrap();
        assert_eq!(likes_of(&db, "m1"), 0);
    }
}
