//! Observable query engine for reactive database subscriptions.
//!
//! Evaluates a query function initially and re-evaluates it whenever relevant
//! [`TableChangeEvent`]s are observed on the [`ChangeBus`]. Results are streamed
//! via [`tokio::sync::watch::Receiver`].

use crate::change_bus::{Table, TableChangeEvent};
use crate::error::DbError;
use crate::Database;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Configuration options for an observable query.
#[derive(Clone, Debug)]
pub struct ObservableOptions {
    /// Debounce window to coalesce rapid consecutive writes. Defaults to 50ms.
    pub debounce: Duration,
    /// Optional account filter. If `Some(pk)`, events targeting a different specific
    /// account are skipped. Events with `affected_account: None` always trigger re-evaluation.
    pub account_filter: Option<String>,
}

impl Default for ObservableOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(50),
            account_filter: None,
        }
    }
}

impl ObservableOptions {
    /// Set a custom debounce duration.
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    /// Filter change events to a specific account pubkey (case-insensitive).
    pub fn with_account(mut self, account: impl Into<String>) -> Self {
        self.account_filter = Some(account.into().trim().to_ascii_lowercase());
        self
    }
}

/// Handle to an active observable query.
pub struct ObservableHandle<T> {
    receiver: watch::Receiver<T>,
    task: tokio::task::JoinHandle<()>,
}

impl<T: Clone> ObservableHandle<T> {
    /// Get the latest evaluated query result.
    pub fn current(&self) -> T {
        self.receiver.borrow().clone()
    }

    /// Obtain a new watch receiver to listen for updates.
    pub fn subscribe(&self) -> watch::Receiver<T> {
        self.receiver.clone()
    }

    /// Borrow the underlying watch receiver.
    pub fn receiver(&self) -> &watch::Receiver<T> {
        &self.receiver
    }

    /// Abort the background observation task.
    pub fn abort(&self) {
        self.task.abort();
    }
}

impl<T> Drop for ObservableHandle<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Database {
    /// Create an observable query that watches one or more tables and publishes updates
    /// via an [`ObservableHandle`].
    ///
    /// The query function `query_fn` is evaluated immediately to provide the initial value.
    /// Whenever a mutation occurs on any of the specified `tables` (matching `options.account_filter`),
    /// the query is automatically re-evaluated after the configured debounce interval.
    pub fn observe<T, F>(
        &self,
        tables: &[Table],
        options: ObservableOptions,
        query_fn: F,
    ) -> Result<ObservableHandle<T>, DbError>
    where
        T: Clone + Send + Sync + 'static,
        F: Fn(&Database) -> Result<T, DbError> + Send + Sync + 'static,
    {
        // Evaluate initial value immediately
        let initial_val = query_fn(self)?;
        let (tx, rx) = watch::channel(initial_val);
        let db = self.clone();
        let query_fn = Arc::new(query_fn);
        let watched_tables: HashSet<Table> = tables.iter().copied().collect();
        let account_filter = options
            .account_filter
            .map(|a| a.trim().to_ascii_lowercase());
        let debounce = options.debounce;
        let tables_fallback = *tables.first().unwrap_or(&Table::Posts);

        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => {
                static SHARED_RT: std::sync::OnceLock<tokio::runtime::Runtime> =
                    std::sync::OnceLock::new();
                let rt = SHARED_RT.get_or_init(|| {
                    tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("shared tokio runtime for db-core observable")
                });
                rt.handle().clone()
            }
        };

        let task = handle.spawn(async move {
            let mut bus_rx = db.subscribe_changes();
            loop {
                // Wait for an event on the change bus
                let event = match bus_rx.recv().await {
                    Ok(evt) => evt,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Lagged means we missed events: trigger re-evaluation unconditionally
                        TableChangeEvent {
                            table: tables_fallback,
                            affected_account: None,
                            timestamp: 0,
                        }
                    }
                };

                // Filter by table
                if !watched_tables.contains(&event.table) {
                    continue;
                }

                // Filter by affected account if set
                if let (Some(filter_pk), Some(evt_pk)) = (&account_filter, &event.affected_account)
                {
                    if !filter_pk.eq_ignore_ascii_case(evt_pk) {
                        continue;
                    }
                }

                // If no subscribers remain, exit the observation loop
                if tx.is_closed() {
                    break;
                }

                // Debounce / coalesce rapid bursts
                tokio::time::sleep(debounce).await;

                // Drain any other pending matching events that arrived during sleep
                while let Ok(burst_evt) = bus_rx.try_recv() {
                    let _ = burst_evt;
                }

                // Check again if subscribers are still active before running query
                if tx.is_closed() {
                    break;
                }

                // Re-evaluate query on a worker thread
                let db_ref = db.clone();
                let q_fn = query_fn.clone();
                let eval_result = tokio::task::spawn_blocking(move || q_fn(&db_ref)).await;

                match eval_result {
                    Ok(Ok(new_val)) => {
                        if tx.send(new_val).is_err() {
                            break; // All receivers closed
                        }
                    }
                    Ok(Err(_)) => {
                        // In case of error (e.g. transient query lock), keep previous state
                    }
                    Err(_) => break, // Join error / task cancelled
                }
            }
        });

        Ok(ObservableHandle { receiver: rx, task })
    }
}
