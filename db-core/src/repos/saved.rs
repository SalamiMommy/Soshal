use crate::Database;
use libsql::params;

pub struct SavedContentRepo<'a> {
    db: &'a Database,
}

impl<'a> SavedContentRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, row: &SavedContentRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO saved_content (kind, id, pubkey, d, media_type, media_url, text_overlay, title, thumbnail, blob_hash, media_size, audience, hashtags, host_ready, created_at, saved_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(kind, id) DO UPDATE SET pubkey=excluded.pubkey, d=excluded.d, media_type=excluded.media_type, media_url=excluded.media_url, text_overlay=excluded.text_overlay, title=excluded.title, thumbnail=excluded.thumbnail, blob_hash=excluded.blob_hash, media_size=excluded.media_size, audience=excluded.audience, hashtags=excluded.hashtags, host_ready=excluded.host_ready, created_at=excluded.created_at, saved_at=excluded.saved_at",
            params![
                row.kind,
                row.id.as_str(),
                row.pubkey.as_str(),
                row.d.as_str(),
                row.media_type.as_str(),
                row.media_url.as_str(),
                row.text_overlay.as_str(),
                row.title.as_str(),
                row.thumbnail.as_str(),
                row.blob_hash.as_str(),
                row.media_size,
                row.audience.as_str(),
                row.hashtags.as_str(),
                row.host_ready as i64,
                row.created_at,
                row.saved_at
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, kind: i64, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM saved_content WHERE kind=?1 AND id=?2",
            params![kind, id],
        )?;
        Ok(())
    }

    pub fn get(
        &self,
        kind: i64,
        id: &str,
    ) -> Result<Option<SavedContentRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT kind, id, pubkey, d, media_type, media_url, text_overlay, title, thumbnail, blob_hash, media_size, audience, hashtags, host_ready, created_at, saved_at FROM saved_content WHERE kind=?1 AND id=?2",
            params![kind, id],
            Self::map_row,
        )
    }

    pub fn set_host_ready(
        &self,
        kind: i64,
        id: &str,
        host_ready: bool,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE saved_content SET host_ready=?1 WHERE kind=?2 AND id=?3",
            params![host_ready as i64, kind, id],
        )?;
        Ok(())
    }

    pub fn list(
        &self,
        kind: i64,
        limit: i64,
    ) -> Result<Vec<SavedContentRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT kind, id, pubkey, d, media_type, media_url, text_overlay, title, thumbnail, blob_hash, media_size, audience, hashtags, host_ready, created_at, saved_at FROM saved_content WHERE kind=?1 ORDER BY saved_at DESC LIMIT ?2",
            params![kind, limit],
            Self::map_row,
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<SavedContentRow> {
        Ok(SavedContentRow {
            kind: r.get(0)?,
            id: r.get(1)?,
            pubkey: r.get(2)?,
            d: r.get(3)?,
            media_type: r.get(4)?,
            media_url: r.get(5)?,
            text_overlay: r.get(6)?,
            title: r.get(7)?,
            thumbnail: r.get(8)?,
            blob_hash: r.get(9)?,
            media_size: r.get(10)?,
            audience: r.get(11)?,
            hashtags: r.get(12)?,
            host_ready: r.get::<i64>(13).map(|v| v != 0)?,
            created_at: r.get(14)?,
            saved_at: r.get(15)?,
        })
    }
}

pub struct SavedContentRow {
    pub kind: i64,
    pub id: String,
    pub pubkey: String,
    pub d: String,
    pub media_type: String,
    pub media_url: String,
    pub text_overlay: String,
    pub title: String,
    pub thumbnail: String,
    pub blob_hash: String,
    pub media_size: i64,
    pub audience: String,
    pub hashtags: String,
    pub host_ready: bool,
    pub created_at: i64,
    pub saved_at: i64,
}

pub struct MusicloudPlaylistRepo<'a> {
    db: &'a Database,
}

impl<'a> MusicloudPlaylistRepo<'a> {
    soshal_repo_new!();

    pub fn create(
        &self,
        id: &str,
        pubkey: &str,
        title: &str,
        is_private: bool,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR IGNORE INTO musicloud_playlists (id, pubkey, title, is_private, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, pubkey, title, is_private as i64, 0],
        )?;
        Ok(())
    }

    pub fn list(
        &self,
        pubkey: &str,
        limit: i64,
    ) -> Result<Vec<PlaylistRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT p.id, p.pubkey, p.title, p.is_private, p.created_at, COUNT(t.track_id) FROM musicloud_playlists p LEFT JOIN musicloud_playlist_tracks t ON t.playlist_id = p.id WHERE p.pubkey=?1 GROUP BY p.id ORDER BY p.created_at DESC LIMIT ?2",
            params![pubkey, limit],
            Self::map_playlist_row,
        )
    }

    pub fn get(&self, id: &str) -> Result<Option<PlaylistRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT p.id, p.pubkey, p.title, p.is_private, p.created_at, COUNT(t.track_id) FROM musicloud_playlists p LEFT JOIN musicloud_playlist_tracks t ON t.playlist_id = p.id WHERE p.id=?1 GROUP BY p.id",
            params![id],
            Self::map_playlist_row,
        )
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE musicloud_playlists SET title=?1 WHERE id=?2",
            params![title, id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM musicloud_playlist_tracks WHERE playlist_id=?1",
            params![id],
        )?;
        crate::query::execute(
            &conn,
            "DELETE FROM musicloud_playlists WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn add_track(&self, track: &PlaylistTrackRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        let position = crate::block_on(async {
            let mut rows = conn
                .query(
                    "SELECT COALESCE(MAX(position)+1, 1) FROM musicloud_playlist_tracks WHERE playlist_id=?1",
                    [track.playlist_id.as_str()],
                )
                .await?;
            let row = rows.next().await?.expect("aggregate query returns one row");
            row.get::<i64>(0)
        })?;
        crate::query::execute(
            &conn,
            "INSERT INTO musicloud_playlist_tracks (playlist_id, track_id, pubkey, d, title, thumbnail, audio_url, blob_hash, media_size, audience, hashtags, created_at, position, added_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(playlist_id, track_id) DO NOTHING",
            params![
                track.playlist_id.as_str(),
                track.track_id.as_str(),
                track.pubkey.as_str(),
                track.d.as_str(),
                track.title.as_str(),
                track.thumbnail.as_str(),
                track.audio_url.as_str(),
                track.blob_hash.as_str(),
                track.media_size,
                track.audience.as_str(),
                track.hashtags.as_str(),
                track.created_at,
                position,
                track.added_at
            ],
        )?;
        Ok(())
    }

    pub fn remove_track(
        &self,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM musicloud_playlist_tracks WHERE playlist_id=?1 AND track_id=?2",
            params![playlist_id, track_id],
        )?;
        Ok(())
    }

    pub fn tracks(
        &self,
        playlist_id: &str,
        limit: i64,
    ) -> Result<Vec<PlaylistTrackRow>, crate::error::DbError> {
        let limit = crate::repos::clamp_limit(limit);
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT playlist_id, track_id, pubkey, d, title, thumbnail, audio_url, blob_hash, media_size, audience, hashtags, created_at, position, added_at FROM musicloud_playlist_tracks WHERE playlist_id=?1 ORDER BY position ASC, added_at ASC LIMIT ?2",
            params![playlist_id, limit],
            Self::map_track_row,
        )
    }

    fn map_playlist_row(r: &libsql::Row) -> libsql::Result<PlaylistRow> {
        Ok(PlaylistRow {
            id: r.get(0)?,
            pubkey: r.get(1)?,
            title: r.get(2)?,
            is_private: r.get::<i64>(3).map(|v| v != 0)?,
            created_at: r.get(4)?,
            track_count: r.get(5)?,
        })
    }

    fn map_track_row(r: &libsql::Row) -> libsql::Result<PlaylistTrackRow> {
        Ok(PlaylistTrackRow {
            playlist_id: r.get(0)?,
            track_id: r.get(1)?,
            pubkey: r.get(2)?,
            d: r.get(3)?,
            title: r.get(4)?,
            thumbnail: r.get(5)?,
            audio_url: r.get(6)?,
            blob_hash: r.get(7)?,
            media_size: r.get(8)?,
            audience: r.get(9)?,
            hashtags: r.get(10)?,
            created_at: r.get(11)?,
            position: r.get(12)?,
            added_at: r.get(13)?,
        })
    }
}

pub struct PlaylistRow {
    pub id: String,
    pub pubkey: String,
    pub title: String,
    pub is_private: bool,
    pub created_at: i64,
    pub track_count: i64,
}

pub struct PlaylistTrackRow {
    pub playlist_id: String,
    pub track_id: String,
    pub pubkey: String,
    pub d: String,
    pub title: String,
    pub thumbnail: String,
    pub audio_url: String,
    pub blob_hash: String,
    pub media_size: i64,
    pub audience: String,
    pub hashtags: String,
    pub created_at: i64,
    pub position: i64,
    pub added_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn saved_upsert_get_host_ready_list_delete() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();
        let repo = SavedContentRepo::new(&db);

        let row = SavedContentRow {
            kind: 31020,
            id: "s1".into(),
            pubkey: "pk".into(),
            d: "d1".into(),
            media_type: "video".into(),
            media_url: "https://example.com/v.mp4".into(),
            text_overlay: "caption".into(),
            title: "Saved Post".into(),
            thumbnail: "https://example.com/t.jpg".into(),
            blob_hash: "bh".into(),
            media_size: 1024,
            audience: "public".into(),
            hashtags: "[]".into(),
            host_ready: false,
            created_at: 1000,
            saved_at: 2000,
        };
        repo.upsert(&row).unwrap();

        let got = repo.get(31020, "s1").unwrap().expect("row present");
        assert_eq!(got.id, "s1");
        assert_eq!(got.title, "Saved Post");
        assert!(!got.host_ready);
        assert_eq!(got.saved_at, 2000);

        repo.set_host_ready(31020, "s1", true).unwrap();
        assert!(repo.get(31020, "s1").unwrap().unwrap().host_ready);

        let list = repo.list(31020, 10).unwrap();
        assert_eq!(list.len(), 1);

        repo.delete(31020, "s1").unwrap();
        assert!(repo.get(31020, "s1").unwrap().is_none());
        assert!(repo.list(31020, 10).unwrap().is_empty());
    }

    #[test]
    fn playlist_crud_and_position_ordering() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();
        let repo = MusicloudPlaylistRepo::new(&db);

        repo.create("p1", "pk", "My Mix", false).unwrap();
        let pls = repo.list("pk", 10).unwrap();
        assert_eq!(pls.len(), 1);
        assert_eq!(pls[0].track_count, 0);

        let track = |track_id: &str, title: &str, created_at: i64| PlaylistTrackRow {
            playlist_id: "p1".into(),
            track_id: track_id.into(),
            pubkey: "pk".into(),
            d: format!("d-{track_id}"),
            title: title.into(),
            thumbnail: String::new(),
            audio_url: format!("https://example.com/{track_id}.mp3"),
            blob_hash: String::new(),
            media_size: 0,
            audience: "public".into(),
            hashtags: "[]".into(),
            created_at,
            position: 0,
            added_at: created_at,
        };

        let t1 = track("t1", "One", 100);
        repo.add_track(&t1).unwrap();
        repo.add_track(&t1).unwrap();
        let t2 = track("t2", "Two", 200);
        repo.add_track(&t2).unwrap();

        let tracks = repo.tracks("p1", 10).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].track_id, "t1");
        assert_eq!(tracks[1].track_id, "t2");
        assert!(tracks[1].position > tracks[0].position);

        repo.rename("p1", "Renamed").unwrap();
        assert_eq!(repo.get("p1").unwrap().unwrap().title, "Renamed");

        repo.remove_track("p1", "t1").unwrap();
        let tracks = repo.tracks("p1", 10).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].track_id, "t2");

        repo.delete("p1").unwrap();
        assert!(repo.list("pk", 10).unwrap().is_empty());
        assert!(repo.tracks("p1", 10).unwrap().is_empty());
    }
}
