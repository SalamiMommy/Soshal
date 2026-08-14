//! Turso configuration and replication status module.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Credentials and settings for connecting to a remote Turso database instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TursoConfig {
    /// Remote Turso database URL (`libsql://...` or `https://...`).
    pub url: String,
    /// Auth bearer token for Turso Cloud access.
    pub auth_token: String,
    /// Whether automatic background sync is enabled.
    pub auto_sync: bool,
    /// Sync interval in seconds (default: 300).
    pub sync_interval_secs: u64,
}

impl TursoConfig {
    pub fn new(url: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            auth_token: auth_token.into(),
            auto_sync: true,
            sync_interval_secs: 300,
        }
    }
}

/// Status of the Turso embedded replica synchronization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TursoSyncStatus {
    /// Whether Turso remote replication is currently configured.
    pub configured: bool,
    /// Current sync status string: "idle", "syncing", "synced", or "error".
    pub status: String,
    /// Timestamp (Unix epoch seconds) of the last successful sync.
    pub last_synced_at: Option<u64>,
    /// Last error message encountered during sync, if any.
    pub last_error: Option<String>,
}

impl Default for TursoSyncStatus {
    fn default() -> Self {
        Self {
            configured: false,
            status: "idle".to_string(),
            last_synced_at: None,
            last_error: None,
        }
    }
}

/// Shared thread-safe state container for Turso sync metadata.
#[derive(Debug, Clone, Default)]
pub struct TursoState {
    inner: Arc<Mutex<TursoSyncStatus>>,
}

impl TursoState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TursoSyncStatus::default())),
        }
    }

    pub fn status(&self) -> TursoSyncStatus {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_configured(&self, configured: bool) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.configured = configured;
    }

    pub fn set_syncing(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.status = "syncing".to_string();
        guard.last_error = None;
    }

    pub fn set_synced(&self, timestamp: u64) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.status = "synced".to_string();
        guard.last_synced_at = Some(timestamp);
        guard.last_error = None;
    }

    pub fn set_error(&self, err: impl Into<String>) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.status = "error".to_string();
        guard.last_error = Some(err.into());
    }
}
