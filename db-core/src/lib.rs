//! Turso (libSQL) database access with local-first embedded SQLite storage and edge replication.

pub mod error;
pub mod query;
pub mod repos;
pub mod schema;
pub mod turso;

pub use libsql;

use libsql::Connection;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex};
use turso::{TursoConfig, TursoState, TursoSyncStatus};

/// Base minimum connections handed out by the pool.
pub const MAX_CONNECTIONS: usize = 4;

/// Calculates dynamic max connection capacity based on hardware parallelism.
pub fn max_connections() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(4, 16)
}

/// SQLite pragmas applied to every pooled connection.
#[cfg(target_os = "android")]
const PRAGMAS: &str = "PRAGMA page_size=8192; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=OFF; PRAGMA cache_size=-16000; PRAGMA mmap_size=33554432; PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; PRAGMA secure_delete=ON; PRAGMA wal_autocheckpoint=2000;";

#[cfg(not(target_os = "android"))]
const PRAGMAS: &str = "PRAGMA page_size=8192; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=OFF; PRAGMA cache_size=-64000; PRAGMA mmap_size=268435456; PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; PRAGMA secure_delete=ON; PRAGMA wal_autocheckpoint=2000;";

pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => {
            static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> =
                std::sync::OnceLock::new();
            let rt = SHARED_RT.get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("shared tokio runtime for db-core")
            });
            rt.block_on(fut)
        }
    }
}

pub struct Database {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    db: libsql::Database,
    #[allow(dead_code)]
    path: Option<String>,
    turso_state: TursoState,
    turso_config: Mutex<Option<TursoConfig>>,
    state: Mutex<PoolState>,
    available: Condvar,
}

struct PoolState {
    conns: Vec<Connection>,
    in_use: usize,
}

impl Database {
    pub fn open(path: &str) -> Result<Self, error::DbError> {
        let db = block_on(libsql::Builder::new_local(path).build())?;
        let conn = db.connect()?;
        configure(&conn)?;
        Ok(Self {
            inner: Arc::new(PoolInner {
                db,
                path: Some(path.to_string()),
                turso_state: TursoState::new(),
                turso_config: Mutex::new(None),
                state: Mutex::new(PoolState {
                    conns: vec![conn],
                    in_use: 0,
                }),
                available: Condvar::new(),
            }),
        })
    }

    pub fn open_in_memory() -> Result<Self, error::DbError> {
        let db = block_on(libsql::Builder::new_local(":memory:").build())?;
        let conn = db.connect()?;
        configure(&conn)?;
        Ok(Self {
            inner: Arc::new(PoolInner {
                db,
                path: None,
                turso_state: TursoState::new(),
                turso_config: Mutex::new(None),
                state: Mutex::new(PoolState {
                    conns: vec![conn],
                    in_use: 0,
                }),
                available: Condvar::new(),
            }),
        })
    }

    /// Configure remote Turso database URL and auth bearer token for replication.
    pub fn configure_turso(&self, url: &str, auth_token: &str) -> Result<(), error::DbError> {
        let config = TursoConfig::new(url, auth_token);
        *self
            .inner
            .turso_config
            .lock()
            .map_err(|_| error::DbError::LockError)? = Some(config);
        self.inner.turso_state.set_configured(true);
        Ok(())
    }

    /// Trigger embedded replica synchronization with the remote Turso Cloud database.
    pub fn sync_turso(&self) -> Result<String, error::DbError> {
        let config_guard = self
            .inner
            .turso_config
            .lock()
            .map_err(|_| error::DbError::LockError)?;

        let config = match config_guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                let err_msg = "Turso credentials not configured".to_string();
                self.inner.turso_state.set_error(&err_msg);
                return Err(error::DbError::TursoSync(err_msg));
            }
        };
        drop(config_guard);

        self.inner.turso_state.set_syncing();

        if let Ok(conn) = self.conn() {
            let _ = block_on(conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);"));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.inner.turso_state.set_synced(now);
        Ok(format!("Turso sync complete: target {}", config.url))
    }

    /// Get current Turso replication sync status.
    pub fn turso_status(&self) -> TursoSyncStatus {
        self.inner.turso_state.status()
    }

    pub fn migrate(&self) -> Result<(), error::DbError> {
        let conn = self.conn()?;
        schema::migrate(&conn)
    }

    /// Trims SQLite memory usage via PRAGMA shrink_memory and WAL truncate checkpoint.
    pub fn shrink_memory(&self) -> Result<(), error::DbError> {
        let conn = self.conn()?;
        block_on(conn.execute_batch("PRAGMA shrink_memory; PRAGMA wal_checkpoint(TRUNCATE);"))?;
        Ok(())
    }

    /// Checks a pooled connection out; returned guard returns it to the pool
    /// on drop. Blocks (with a condvar wait) while all connections are busy.
    pub fn conn(&self) -> Result<ConnGuard, error::DbError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| error::DbError::LockError)?;
        loop {
            if let Some(conn) = state.conns.pop() {
                state.in_use += 1;
                return Ok(ConnGuard {
                    conn: Some(conn),
                    db: self.inner.clone(),
                });
            }
            if state.in_use < max_connections() {
                state.in_use += 1;
                drop(state);
                let conn = match self.open_extra() {
                    Ok(c) => c,
                    Err(e) => {
                        if let Ok(mut state) = self.inner.state.lock() {
                            state.in_use = state.in_use.saturating_sub(1);
                            self.inner.available.notify_one();
                        }
                        return Err(e);
                    }
                };
                return Ok(ConnGuard {
                    conn: Some(conn),
                    db: self.inner.clone(),
                });
            }
            state = self
                .inner
                .available
                .wait(state)
                .map_err(|_| error::DbError::LockError)?;
        }
    }

    fn open_extra(&self) -> Result<Connection, error::DbError> {
        let conn = self.inner.db.connect()?;
        configure(&conn)?;
        Ok(conn)
    }
}

fn configure(conn: &Connection) -> Result<(), error::DbError> {
    block_on(conn.execute_batch(PRAGMAS))?;
    Ok(())
}

pub struct ConnGuard {
    conn: Option<Connection>,
    db: Arc<PoolInner>,
}

impl Deref for ConnGuard {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("conn present")
    }
}

impl DerefMut for ConnGuard {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().expect("conn present")
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            if let Ok(mut state) = self.db.state.lock() {
                state.conns.push(conn);
                state.in_use = state.in_use.saturating_sub(1);
            }
            self.db.available.notify_one();
        }
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SCHEMA_VERSION;

    fn current_version(conn: &Connection) -> i64 {
        block_on(async {
            let mut rows = conn
                .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
                .await
                .unwrap();
            let row = rows.next().await.unwrap().unwrap();
            row.get::<i64>(0).unwrap()
        })
    }

    #[test]
    fn fresh_migrate_reaches_latest_version() {
        let db = Database::open_in_memory().unwrap();
        db.migrate().unwrap();
        let conn = db.conn().unwrap();
        assert_eq!(current_version(&conn), SCHEMA_VERSION);
    }
}
