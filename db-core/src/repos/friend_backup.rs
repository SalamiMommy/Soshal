use crate::Database;
use libsql::params;

pub struct FriendBackupRepo<'a> {
    db: &'a Database,
}

impl<'a> FriendBackupRepo<'a> {
    soshal_repo_new!();

    pub fn upsert(&self, b: &FriendBackupRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO friend_backups (user_pubkey, encrypted_data, updated_at) VALUES (?1,?2,?3) ON CONFLICT(user_pubkey) DO UPDATE SET encrypted_data=excluded.encrypted_data, updated_at=excluded.updated_at",
            params![b.user_pubkey.as_str(), b.encrypted_data.as_str(), b.updated_at],
        )?;
        Ok(())
    }

    pub fn get(&self, user_pubkey: &str) -> Result<Option<FriendBackupRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT user_pubkey, encrypted_data, updated_at FROM friend_backups WHERE user_pubkey=?1",
            params![user_pubkey],
            Self::map_row,
        )
    }

    pub fn delete(&self, user_pubkey: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM friend_backups WHERE user_pubkey=?1",
            params![user_pubkey],
        )?;
        Ok(())
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<FriendBackupRow> {
        Ok(FriendBackupRow {
            user_pubkey: r.get(0)?,
            encrypted_data: r.get(1)?,
            updated_at: r.get(2)?,
        })
    }
}

pub struct FriendBackupRow {
    pub user_pubkey: String,
    pub encrypted_data: String,
    pub updated_at: i64,
}
