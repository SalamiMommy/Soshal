//! In-process reactive change bus for SQLite table mutations.
//!
//! Provides lightweight Change Data Capture (CDC) notification via
//! `tokio::sync::broadcast`. When repositories commit writes or deletions,
//! they emit a [`TableChangeEvent`]. Stream sinks and reactive subscribers
//! can listen to these events to invalidate or re-evaluate live queries.

use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::broadcast;

/// Database tables that support reactive change notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Table {
    Posts,
    Reactions,
    Messages,
    Notifications,
    Profiles,
    Blocks,
    Bookmarks,
    Settings,
    Relays,
}

/// An event describing a committed mutation on a database table.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableChangeEvent {
    /// The table that underwent mutation.
    pub table: Table,
    /// Optional account pubkey directly associated with the change (e.g. author or recipient).
    pub affected_account: Option<String>,
    /// Unix timestamp in seconds when the event was recorded.
    pub timestamp: i64,
}

/// Default broadcast channel capacity for table change notifications.
pub const DEFAULT_CHANGE_BUS_CAPACITY: usize = 1024;

/// In-process broadcast bus for table mutation events.
#[derive(Debug)]
pub struct ChangeBus {
    sender: broadcast::Sender<TableChangeEvent>,
    last_event_ts: AtomicI64,
}

impl Default for ChangeBus {
    fn default() -> Self {
        Self::new(DEFAULT_CHANGE_BUS_CAPACITY)
    }
}

impl ChangeBus {
    /// Create a new `ChangeBus` with the specified buffer capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(16));
        Self {
            sender,
            last_event_ts: AtomicI64::new(0),
        }
    }

    /// Broadcast a table change event to all active subscribers.
    ///
    /// If there are no active subscribers, the event is harmlessly dropped.
    pub fn notify(&self, table: Table, affected_account: Option<String>) {
        let now = soshal_common_core::format::now_secs();
        self.last_event_ts.store(now, Ordering::Relaxed);
        let event = TableChangeEvent {
            table,
            affected_account,
            timestamp: now,
        };
        // broadcast::send returns Err when there are 0 active receivers, which is normal.
        let _ = self.sender.send(event);
    }

    /// Subscribe to the broadcast stream of table mutation events.
    pub fn subscribe(&self) -> broadcast::Receiver<TableChangeEvent> {
        self.sender.subscribe()
    }

    /// Returns the number of currently active subscribers.
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Returns the timestamp (seconds) of the most recent notification, or 0 if none.
    pub fn last_event_ts(&self) -> i64 {
        self.last_event_ts.load(Ordering::Relaxed)
    }
}
