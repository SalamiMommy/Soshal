//! Freenet backend: contract Get/Put over the local Freenet node's WebSocket
//! gateway (gated on a reachable node; each contract put counts as one hop).

use crate::backends::{BackendKind, MeshBackend};
use crate::envelope::MAX_ENVELOPE_BYTES;
use soshal_network_core::freenet_websocket::{ContractState, FreenetWebSocketClient};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("freenet backend tokio runtime")
    })
}

fn block_on_async<F, R>(fut: F) -> R
where
    F: std::future::Future<Output = R>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        runtime().block_on(fut)
    }
}

pub struct FreenetBackend {
    /// Shared client slot. `broadcast` hands payloads to a worker thread that
    /// does the blocking `put_contract`, while `recv` reconnects on the poll
    /// thread, so both must observe the *same* client — a `reconnect()` that
    /// swapped in a fresh `FreenetWebSocketClient` would otherwise leave the
    /// worker holding a permanently dead socket.
    client: Arc<Mutex<Option<FreenetWebSocketClient>>>,
    /// Bounded queue of pending puts, drained by the worker spawned in
    /// `start()`. `broadcast` only enqueues.
    outbox: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// Payloads lost to a full outbox, and puts the worker attempted that the
    /// node rejected or timed out. Counted, never dropped silently.
    dropped: Arc<std::sync::atomic::AtomicU64>,
    put_failures: Arc<std::sync::atomic::AtomicU64>,
    url: String,
    auth_token: String,
    contract_key: String,
    received: Arc<Mutex<VecDeque<Vec<u8>>>>,
    last_state: Option<Vec<u8>>,
    started: bool,
    connected: bool,
    backoff: std::time::Duration,
    reconnect_at: std::time::Instant,
}

/// Outbox depth. A put costs up to `PUT_TIMEOUT`, so this only fills when the
/// Freenet node is far slower than the event rate; it exists to bound memory,
/// not to be routinely hit.
const OUTBOX_CAPACITY: usize = 256;
const PUT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl Default for FreenetBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl FreenetBackend {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            outbox: None,
            worker: None,
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            put_failures: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            url: "ws://127.0.0.1:7509".to_string(),
            auth_token: String::new(),
            contract_key: "soshal-mesh-v1".to_string(),
            received: Arc::new(Mutex::new(VecDeque::new())),
            last_state: None,
            started: false,
            connected: false,
            backoff: std::time::Duration::from_secs(1),
            reconnect_at: std::time::Instant::now(),
        }
    }

    /// Outbox overflows so far. Surfaced by the relay status so a saturated
    /// node is visible instead of silently losing re-broadcasts.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Puts the worker attempted that the node rejected or timed out.
    pub fn put_failures(&self) -> u64 {
        self.put_failures.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Dedup helper: returns the incoming state only when it differs from the
    /// current cursor, updating the cursor either way.
    fn state_changed(current: &mut Option<Vec<u8>>, incoming: Vec<u8>) -> Option<Vec<u8>> {
        if current.as_ref() == Some(&incoming) {
            return None;
        }
        *current = Some(incoming.clone());
        Some(incoming)
    }

    fn client_snapshot(&self) -> Option<FreenetWebSocketClient> {
        self.client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Spawns the outbox worker. Idempotent — a `start()` on an already-started
    /// backend must not leave a second thread draining the queue.
    fn spawn_worker(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(OUTBOX_CAPACITY);
        let client = Arc::clone(&self.client);
        let contract_key = self.contract_key.clone();
        let put_failures = Arc::clone(&self.put_failures);
        self.outbox = Some(tx);
        self.worker = Some(std::thread::spawn(move || {
            // Runs until every sender drops, i.e. until `stop()` or `Drop`.
            for payload in rx {
                let Some(client) = client.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
                    // Not connected: nothing to put to. Counted, so a dead
                    // node stays distinguishable from a healthy one.
                    put_failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    continue;
                };
                let state = ContractState {
                    key: contract_key.clone(),
                    state: payload,
                    contract_code: None,
                };
                let result = block_on_async(async {
                    tokio::time::timeout(PUT_TIMEOUT, client.put_contract(state, false))
                        .await
                        .map_err(|_| "freenet put timed out".to_string())?
                });
                if result.is_err() {
                    put_failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }));
    }

    /// Closes the outbox and waits for the worker to finish its current put.
    /// Ordering matters: the node must still be connected while the worker
    /// drains, so this runs before the disconnect.
    fn stop_worker(&mut self) {
        // Dropping the last sender ends the worker's `for` loop.
        self.outbox = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    fn reconnect(&mut self) -> bool {
        let client = FreenetWebSocketClient::new(self.url.clone(), self.auth_token.clone());
        match block_on_async(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), client.connect())
                .await
                .map_err(|_| "freenet connect timed out".to_string())?
        }) {
            Ok(()) => {
                *self.client.lock().unwrap_or_else(|e| e.into_inner()) = Some(client);
                self.connected = true;
                self.backoff = std::time::Duration::from_secs(1);
                true
            }
            Err(_) => {
                *self.client.lock().unwrap_or_else(|e| e.into_inner()) = None;
                false
            }
        }
    }
}

impl Drop for FreenetBackend {
    fn drop(&mut self) {
        // Close the channel so the worker exits after its current put. The
        // handle is dropped (detached) rather than joined: tearing a node down
        // mid-put should not block the caller for up to PUT_TIMEOUT.
        self.outbox = None;
    }
}

impl MeshBackend for FreenetBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Freenet
    }
    fn start(&mut self) -> Result<(), String> {
        let client = FreenetWebSocketClient::new(self.url.clone(), self.auth_token.clone());
        block_on_async(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), client.connect())
                .await
                .map_err(|_| "freenet connect timed out".to_string())?
        })?;
        *self.client.lock().unwrap_or_else(|e| e.into_inner()) = Some(client);
        self.started = true;
        self.connected = true;
        self.backoff = std::time::Duration::from_secs(1);
        self.reconnect_at = std::time::Instant::now();
        self.spawn_worker();
        Ok(())
    }
    fn stop(&mut self) {
        self.started = false;
        self.connected = false;
        // Drain before disconnecting: the worker still needs a live socket.
        self.stop_worker();
        let client = self.client.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(client) = client {
            let _ = block_on_async(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), client.disconnect()).await
            });
        }
    }
    fn running(&self) -> bool {
        self.started
    }
    /// Enqueues the payload for the outbox worker and returns immediately.
    ///
    /// This used to perform the contract put inline, which blocked the caller
    /// for up to 5 s per event. The caller is `RelayNode::re_broadcast`, on the
    /// mesh relay poll thread — so a slow or wedged Freenet node stalled ingest
    /// for *every* backend, and I2P/Reticulum inbound queues were not even
    /// drained while a put was outstanding. Enqueueing keeps the poll loop
    /// non-blocking; the put itself happens on the worker.
    fn broadcast(&mut self, payload: Vec<u8>) -> Result<usize, String> {
        if !self.started {
            return Ok(0);
        }
        if payload.len() > MAX_ENVELOPE_BYTES {
            return Err("freenet broadcast: payload exceeds MAX_ENVELOPE_BYTES".to_string());
        }
        let Some(tx) = self.outbox.as_ref() else {
            return Err("freenet outbox not running".to_string());
        };
        match tx.try_send(payload) {
            Ok(()) => Ok(1),
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                // Counted, not silent: a node far slower than the event rate is
                // a real condition the operator needs to see.
                self.dropped
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err("freenet outbox full".to_string())
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                Err("freenet outbox worker gone".to_string())
            }
        }
    }
    fn recv(&mut self) -> Vec<Vec<u8>> {
        if self.started {
            if !self.connected && std::time::Instant::now() >= self.reconnect_at {
                self.connected = self.reconnect();
                let delay = self.backoff;
                self.reconnect_at = std::time::Instant::now() + delay;
                self.backoff = std::cmp::min(delay * 2, std::time::Duration::from_secs(30));
            }
            if self.connected {
                if let Some(client) = self.client_snapshot() {
                    if let Ok(state) = block_on_async(async {
                        tokio::time::timeout(
                            std::time::Duration::from_secs(5),
                            client.get_contract(&self.contract_key, false),
                        )
                        .await
                        .map_err(|_| "freenet get timed out".to_string())?
                    }) {
                        if let Some(bytes) = Self::state_changed(&mut self.last_state, state.state)
                        {
                            self.received
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push_back(bytes);
                        }
                    } else {
                        self.connected = false;
                    }
                }
            }
        }
        let mut received = self.received.lock().unwrap_or_else(|e| e.into_inner());
        received.drain(..).collect()
    }
    fn peers(&self) -> Vec<String> {
        if self.started {
            vec![self.url.clone()]
        } else {
            Vec::new()
        }
    }
    fn peers_count(&self) -> usize {
        if self.started {
            1
        } else {
            0
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn health(&self) -> serde_json::Value {
        serde_json::json!({
            "freenet_outbox_dropped": self.dropped(),
            "freenet_put_failures": self.put_failures(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constructor_defaults() {
        let backend = FreenetBackend::new();
        assert_eq!(backend.url, "ws://127.0.0.1:7509");
        assert!(backend.auth_token.is_empty());
        assert_eq!(backend.contract_key, "soshal-mesh-v1");
        assert!(!backend.running());
    }

    #[test]
    fn test_broadcast_requires_started() {
        let mut backend = FreenetBackend::new();
        assert_eq!(backend.broadcast(vec![1, 2, 3]), Ok(0));
    }

    #[test]
    fn test_poll_recv_deduplicates_state() {
        let mut current: Option<Vec<u8>> = None;
        assert_eq!(
            FreenetBackend::state_changed(&mut current, vec![1, 2]),
            Some(vec![1, 2])
        );
        assert_eq!(
            FreenetBackend::state_changed(&mut current, vec![1, 2]),
            None
        );
        assert_eq!(
            FreenetBackend::state_changed(&mut current, vec![3]),
            Some(vec![3])
        );
    }

    /// Puts a backend into the started-with-worker state without a live
    /// Freenet node, so the outbox path is testable in CI. With no client
    /// installed the worker counts every payload as a failed put and returns
    /// immediately — no network, no timeout.
    fn started_without_node() -> FreenetBackend {
        let mut backend = FreenetBackend::new();
        backend.started = true;
        backend.connected = true;
        backend.spawn_worker();
        backend
    }

    /// Polls until [cond] holds or the budget expires. Worker-driven
    /// assertions need a real hand-off, not a fixed sleep.
    fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        cond()
    }

    #[test]
    fn broadcast_enqueues_without_blocking_and_worker_counts_failures() {
        let mut backend = started_without_node();
        let started = std::time::Instant::now();

        // The whole point of the outbox: `broadcast` returns immediately rather
        // than performing the (up to 5 s) put inline.
        for i in 0..8u8 {
            assert_eq!(backend.broadcast(vec![i]).unwrap(), 1);
        }
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "broadcast blocked the caller for {:?}",
            started.elapsed()
        );

        assert!(
            wait_for(|| backend.put_failures() == 8),
            "worker counted {} of 8 failed puts",
            backend.put_failures()
        );
        // An enqueued payload is not an overflow, so nothing was dropped.
        assert_eq!(backend.dropped(), 0);
    }

    #[test]
    fn oversized_payload_is_rejected_before_enqueueing() {
        let mut backend = started_without_node();
        let too_big = vec![0u8; MAX_ENVELOPE_BYTES + 1];

        assert!(backend.broadcast(too_big).is_err());
        // Rejected at the door: it must not reach the worker.
        assert!(!wait_for(|| backend.put_failures() > 0));
    }

    #[test]
    fn health_reports_both_counters() {
        let mut backend = started_without_node();
        backend.broadcast(vec![1]).unwrap();
        assert!(wait_for(|| backend.put_failures() == 1));

        let health = backend.health();
        assert_eq!(health["freenet_put_failures"], 1);
        assert_eq!(health["freenet_outbox_dropped"], 0);
    }

    #[test]
    fn stop_joins_the_worker_so_counters_stop_growing() {
        let mut backend = started_without_node();
        backend.broadcast(vec![1]).unwrap();
        assert!(wait_for(|| backend.put_failures() == 1));

        backend.stop();
        let settled = backend.put_failures();
        // `stop()` clears `started`, so a post-stop broadcast is the same
        // no-op as a pre-start one: nothing reaches the (now joined) worker.
        assert_eq!(backend.broadcast(vec![2]), Ok(0));
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(backend.put_failures(), settled);
    }

    #[test]
    fn broadcast_before_start_is_a_noop_and_does_not_count_as_dropped() {
        let mut backend = FreenetBackend::new();

        assert_eq!(backend.broadcast(vec![1, 2, 3]), Ok(0));
        assert_eq!(backend.dropped(), 0);
        assert_eq!(backend.put_failures(), 0);
        assert!(backend.health()["freenet_outbox_dropped"].is_u64());
    }
}
