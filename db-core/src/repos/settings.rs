use crate::error::DbError;
use crate::Database;
use libsql::params;
use std::collections::HashMap;

pub struct SettingsRepo<'a> {
    db: &'a Database,
}

impl<'a> SettingsRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.db.conn()?;
        crate::query::query_first(
            &conn,
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
    }

    pub fn get_all(&self) -> Result<Vec<(String, String)>, DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT key, value FROM settings ORDER BY key",
            (),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    }

    pub fn get_many(&self, keys: &[&str]) -> Result<HashMap<String, String>, DbError> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.db.conn()?;
        let placeholders = vec!["?"; keys.len()].join(", ");
        let sql = format!("SELECT key, value FROM settings WHERE key IN ({placeholders})");
        let rows = crate::query::query(
            &conn,
            &sql,
            libsql::params_from_iter(keys.iter().copied()),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(rows.into_iter().collect())
    }

    pub fn delete(&self, key: &str) -> Result<(), DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }
}
