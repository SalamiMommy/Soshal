use std::fmt;

#[derive(Debug)]
pub enum DbError {
    Sqlite(libsql::Error),
    LockError,
    Migration(String),
    NotFound,
    /// Relay content rejected by size caps.
    Oversized(String),
    /// Operation not permitted for the current identity.
    Forbidden(String),
    /// Turso database sync / replication error.
    TursoSync(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "sqlite: {}", e),
            DbError::LockError => write!(f, "database lock"),
            DbError::Migration(s) => write!(f, "migration: {}", s),
            DbError::NotFound => write!(f, "not found"),
            DbError::Oversized(s) => write!(f, "oversized: {}", s),
            DbError::Forbidden(s) => write!(f, "forbidden: {}", s),
            DbError::TursoSync(s) => write!(f, "turso sync: {}", s),
        }
    }
}

impl std::error::Error for DbError {}

impl From<libsql::Error> for DbError {
    fn from(e: libsql::Error) -> Self {
        DbError::Sqlite(e)
    }
}
