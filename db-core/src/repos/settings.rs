use crate::error::DbError;
use crate::Database;
use libsql::params;
use std::collections::HashMap;

pub struct SettingsRepo<'a> {
    db: &'a Database,
}

impl<'a> SettingsRepo<'a> {
    soshal_repo_new!();

    pub fn set(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        self.db
            .notify_change(crate::change_bus::Table::Settings, None);
        Ok(())
    }

    /// Set several keys in one statement.
    ///
    /// A multi-row upsert is atomic on its own, so this needs no explicit
    /// transaction wrapper — and it notifies the change bus once instead of
    /// once per key. The sync engine used to write its three watermarks with
    /// three separate `set` calls on every flush interval, three connections
    /// and three notifications, whether or not anything had advanced.
    ///
    /// The rows are variable-length, so the `VALUES` list and the bind vector
    /// are both built at runtime. SQLite numbers parameters by order of first
    /// appearance, one number per token, so row *i* binds to `?2i+1`/`?2i+2`
    /// and `binds` is in that same interleaved order.
    ///
    /// No-op on an empty slice.
    pub fn set_many(&self, pairs: &[(&str, &str)]) -> Result<(), DbError> {
        if pairs.is_empty() {
            return Ok(());
        }
        let mut placeholders = String::new();
        let mut binds: Vec<libsql::Value> = Vec::with_capacity(pairs.len() * 2);
        for (i, (k, v)) in pairs.iter().enumerate() {
            if i > 0 {
                placeholders.push_str(", ");
            }
            let base = i * 2;
            placeholders.push_str(&format!("(?{}, ?{})", base + 1, base + 2));
            binds.push(libsql::Value::from(*k));
            binds.push(libsql::Value::from(*v));
        }
        let sql = format!(
            "INSERT INTO settings (key, value) VALUES {placeholders} \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value"
        );
        let conn = self.db.conn()?;
        crate::query::execute(&conn, &sql, libsql::params_from_iter(binds))?;
        self.db
            .notify_change(crate::change_bus::Table::Settings, None);
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
        self.db
            .notify_change(crate::change_bus::Table::Settings, None);
        Ok(())
    }
}
