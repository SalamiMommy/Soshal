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

pub struct FreenetBackend {
    client: Option<FreenetWebSocketClient>,
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

impl Default for FreenetBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl FreenetBackend {
    pub fn new() -> Self {
        Self {
            client: None,
            url: "ws://127.0.0.1:8888".to_string(),
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

    /// Dedup helper: returns the incoming state only when it differs from the
    /// current cursor, updating the cursor either way.
    fn state_changed(current: &mut Option<Vec<u8>>, incoming: Vec<u8>) -> Option<Vec<u8>> {
        if current.as_ref() == Some(&incoming) {
            return None;
        }
        *current = Some(incoming.clone());
        Some(incoming)
    }

    fn reconnect(&mut self) -> bool {
        let client = FreenetWebSocketClient::new(self.url.clone(), self.auth_token.clone());
        match runtime().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), client.connect())
                .await
                .map_err(|_| "freenet connect timed out".to_string())?
        }) {
            Ok(()) => {
                self.client = Some(client);
                self.connected = true;
                self.backoff = std::time::Duration::from_secs(1);
                true
            }
            Err(_) => {
                self.client = None;
                false
            }
        }
    }
}

impl MeshBackend for FreenetBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Freenet
    }
    fn start(&mut self) -> Result<(), String> {
        let client = FreenetWebSocketClient::new(self.url.clone(), self.auth_token.clone());
        runtime().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), client.connect())
                .await
                .map_err(|_| "freenet connect timed out".to_string())?
        })?;
        self.client = Some(client);
        self.started = true;
        self.connected = true;
        self.backoff = std::time::Duration::from_secs(1);
        self.reconnect_at = std::time::Instant::now();
        Ok(())
    }
    fn stop(&mut self) {
        self.started = false;
        self.connected = false;
        if let Some(client) = self.client.take() {
            let _ = runtime().block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), client.disconnect()).await
            });
        }
    }
    fn running(&self) -> bool {
        self.started
    }
    fn broadcast(&mut self, payload: Vec<u8>) -> Result<usize, String> {
        if !self.started {
            return Ok(0);
        }
        if payload.len() > MAX_ENVELOPE_BYTES {
            return Err("freenet broadcast: payload exceeds MAX_ENVELOPE_BYTES".to_string());
        }
        let client = self.client.as_ref().ok_or("freenet backend not started")?;
        let state = ContractState {
            key: self.contract_key.clone(),
            state: payload,
            contract_code: None,
        };
        runtime().block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.put_contract(state, false),
            )
            .await
            .map_err(|_| "freenet put timed out".to_string())?
        })?;
        Ok(1)
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
                if let Some(client) = self.client.as_ref() {
                    if let Ok(state) = runtime().block_on(async {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constructor_defaults() {
        let backend = FreenetBackend::new();
        assert_eq!(backend.url, "ws://127.0.0.1:8888");
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
}
