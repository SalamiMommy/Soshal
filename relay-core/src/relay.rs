//! The relay node: store-and-forward gossip engine over mesh backends.
//!
//! Flood-style: published events are broadcast at hop 0; inbound envelopes
//! are deduped by event id and re-broadcast once at hop+1 (ceiling
//! MAX_HOP_COUNT). Delivered payloads are queued for the app to drain.

use std::collections::{HashSet, VecDeque};

use crate::backends::{BackendKind, MeshBackend};
use crate::envelope::MeshEnvelope;

/// Recent-event ring buffer size (re-broadcastable history).
const RECENT_CAPACITY: usize = 10_000;
/// Seen-id dedup set cap.
const SEEN_CAPACITY: usize = 100_000;
/// App-delivery queue cap.
const DELIVERED_CAPACITY: usize = 4_096;

/// Flood gossip relay node.
pub struct RelayNode {
    backends: Vec<Box<dyn MeshBackend>>,
    recent: VecDeque<MeshEnvelope>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    delivered: VecDeque<Vec<u8>>,
    published: u64,
    received: u64,
    running: bool,
}

impl Default for RelayNode {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayNode {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            recent: VecDeque::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            delivered: VecDeque::new(),
            published: 0,
            received: 0,
            running: false,
        }
    }

    pub fn add_backend(&mut self, backend: Box<dyn MeshBackend>) {
        self.backends.push(backend);
    }

    /// Starts all backends. Returns the number started.
    pub fn start(&mut self) -> Result<usize, String> {
        let mut started = 0usize;
        for backend in self.backends.iter_mut() {
            if backend.start().is_ok() {
                started += 1;
            }
        }
        self.running = started > 0;
        if started == 0 {
            return Err("no mesh backend started".to_string());
        }
        Ok(started)
    }

    pub fn stop(&mut self) {
        for backend in self.backends.iter_mut() {
            backend.stop();
        }
        self.running = false;
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Publishes a signed event payload to the mesh at hop 0.
    /// Returns total peer count handed the envelope.
    pub fn publish(
        &mut self,
        event_id: &str,
        kind: u16,
        author: &str,
        created_at: u64,
        payload: Vec<u8>,
    ) -> Result<usize, String> {
        if !self.running {
            return Err("relay node not running".to_string());
        }
        if payload.len() > crate::envelope::MAX_PAYLOAD_BYTES {
            return Err("payload exceeds mesh cap".to_string());
        }
        let env = MeshEnvelope::new(
            event_id.to_string(),
            kind,
            author.to_string(),
            created_at,
            payload,
        );
        self.note_seen(env.event_id.clone());
        self.recent.push_back(env.clone());
        if self.recent.len() > RECENT_CAPACITY {
            self.recent.pop_front();
        }
        let mut total = 0usize;
        for backend in self.backends.iter_mut() {
            if backend.running() {
                total += backend.broadcast(env.to_bytes()).unwrap_or(0);
            }
        }
        self.published += 1;
        Ok(total)
    }

    /// Polls all backends for inbound payloads: dedup, re-broadcast at
    /// hop+1, queue for app delivery. Returns the number of new envelopes.
    pub fn poll(&mut self) -> usize {
        let mut inbound: Vec<(usize, Vec<Vec<u8>>)> = Vec::new();
        for (idx, backend) in self.backends.iter_mut().enumerate() {
            let payloads = backend.recv();
            if !payloads.is_empty() {
                inbound.push((idx, payloads));
            }
        }
        let mut new_count = 0usize;
        for (idx, payloads) in inbound {
            for payload in payloads {
                let Some(env) = MeshEnvelope::from_bytes(&payload) else {
                    continue;
                };
                if self.seen.contains(&env.event_id) {
                    continue;
                }
                self.note_seen(env.event_id.clone());
                self.received += 1;
                new_count += 1;
                self.delivered.push_back(env.payload.clone());
                while self.delivered.len() > DELIVERED_CAPACITY {
                    self.delivered.pop_front();
                }
                self.recent.push_back(env.clone());
                while self.recent.len() > RECENT_CAPACITY {
                    self.recent.pop_front();
                }
                if let Some(next) = env.increment_hop() {
                    self.re_broadcast(next, idx);
                }
            }
        }
        new_count
    }

    /// Drains payloads queued for the app.
    pub fn drain_delivered(&mut self) -> Vec<Vec<u8>> {
        self.delivered.drain(..).collect()
    }

    /// Per-backend peer counts as `(kind, count)`.
    pub fn peers(&self) -> Vec<(BackendKind, usize)> {
        self.backends
            .iter()
            .map(|b| (b.kind(), b.peers_count()))
            .collect()
    }

    /// JSON status: `{running, peers:{kind:n}, published, received, delivered}`.
    pub fn status(&self) -> String {
        let mut peers = serde_json::Map::new();
        for (kind, count) in self.peers() {
            peers.insert(kind.as_str().to_string(), serde_json::Value::from(count));
        }
        serde_json::json!({
            "running": self.running,
            "peers": peers,
            "published": self.published,
            "received": self.received,
            "delivered": self.delivered.len(),
        })
        .to_string()
    }

    fn re_broadcast(&mut self, env: MeshEnvelope, source_idx: usize) {
        let bytes = env.to_bytes();
        for (idx, backend) in self.backends.iter_mut().enumerate() {
            if idx != source_idx && backend.running() {
                let _ = backend.broadcast(bytes.clone());
            }
        }
    }

    fn note_seen(&mut self, id: String) {
        if self.seen.insert(id.clone()) {
            self.seen_order.push_back(id);
            while self.seen_order.len() > SEEN_CAPACITY {
                if let Some(oldest) = self.seen_order.pop_front() {
                    self.seen.remove(&oldest);
                }
            }
        }
    }

    /// Connects an I2P peer by destination hash on the I2P backend
    /// (no-op when no I2P backend is registered).
    pub fn connect_i2p(&mut self, destination: &str) -> Result<(), String> {
        for backend in self.backends.iter_mut() {
            if backend.kind() == BackendKind::I2p {
                let i2p = backend
                    .as_any_mut()
                    .downcast_mut::<crate::backends::i2p::I2pBackend>()
                    .ok_or("i2p backend downcast failed")?;
                return i2p.connect(destination);
            }
        }
        Err("no i2p backend registered".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::MeshBackend;
    use crate::envelope::MAX_HOP_COUNT;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockState {
        running: bool,
        outbox: Vec<Vec<u8>>,
        inbox: Vec<Vec<u8>>,
        peer_count: usize,
    }

    struct MockBackend {
        kind: BackendKind,
        state: Arc<Mutex<MockState>>,
    }

    impl MockBackend {
        fn new(kind: BackendKind, state: Arc<Mutex<MockState>>) -> Self {
            Self { kind, state }
        }
    }

    impl MeshBackend for MockBackend {
        fn kind(&self) -> BackendKind {
            self.kind
        }
        fn start(&mut self) -> Result<(), String> {
            self.state.lock().unwrap().running = true;
            Ok(())
        }
        fn stop(&mut self) {
            self.state.lock().unwrap().running = false;
        }
        fn running(&self) -> bool {
            self.state.lock().unwrap().running
        }
        fn broadcast(&mut self, payload: Vec<u8>) -> Result<usize, String> {
            self.state.lock().unwrap().outbox.push(payload);
            Ok(self.state.lock().unwrap().peer_count)
        }
        fn recv(&mut self) -> Vec<Vec<u8>> {
            self.state.lock().unwrap().inbox.drain(..).collect()
        }
        fn peers(&self) -> Vec<String> {
            vec!["peer-a".to_string(), "peer-b".to_string()]
        }
        fn peers_count(&self) -> usize {
            self.state.lock().unwrap().peer_count
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn mock(kind: BackendKind, peer_count: usize) -> (MockBackend, Arc<Mutex<MockState>>) {
        let state = Arc::new(Mutex::new(MockState {
            peer_count,
            ..Default::default()
        }));
        (MockBackend::new(kind, state.clone()), state)
    }

    fn env_bytes(hop: u8) -> Vec<u8> {
        let mut env = MeshEnvelope::new(
            "evt-1".to_string(),
            1,
            "author".to_string(),
            0,
            b"hi".to_vec(),
        );
        env.hop_count = hop;
        env.to_bytes()
    }

    #[test]
    fn test_publish_broadcasts_to_all_backends() {
        let mut node = RelayNode::new();
        node.add_backend(Box::new(mock(BackendKind::Reticulum, 2).0));
        node.add_backend(Box::new(mock(BackendKind::I2p, 2).0));
        node.start().unwrap();
        let total = node
            .publish("evt-1", 1, "author", 0, b"hi".to_vec())
            .unwrap();
        assert_eq!(total, 4);
        assert!(node.running());
    }

    #[test]
    fn test_publish_requires_running() {
        let mut node = RelayNode::new();
        node.add_backend(Box::new(mock(BackendKind::Reticulum, 2).0));
        assert!(node.publish("x", 1, "a", 0, b"y".to_vec()).is_err());
    }

    #[test]
    fn test_publish_rejects_oversized_payload() {
        let mut node = RelayNode::new();
        node.add_backend(Box::new(mock(BackendKind::Reticulum, 2).0));
        node.start().unwrap();
        assert!(node
            .publish(
                "x",
                1,
                "a",
                0,
                vec![0u8; crate::envelope::MAX_PAYLOAD_BYTES + 1]
            )
            .is_err());
    }

    #[test]
    fn test_poll_delivers_and_rebroadcasts_to_other_backends() {
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        let (b1, s1) = mock(BackendKind::I2p, 2);
        node.add_backend(Box::new(b0));
        node.add_backend(Box::new(b1));
        node.start().unwrap();
        s0.lock().unwrap().inbox.push(env_bytes(0));
        assert_eq!(node.poll(), 1);
        assert_eq!(node.drain_delivered(), vec![b"hi".to_vec()]);
        let outbox = s1.lock().unwrap().outbox.clone();
        assert_eq!(outbox.len(), 1);
        let env = MeshEnvelope::from_bytes(&outbox[0]).unwrap();
        assert_eq!(env.hop_count, 1);
    }

    #[test]
    fn test_poll_dedups_by_event_id() {
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        node.add_backend(Box::new(b0));
        node.start().unwrap();
        s0.lock().unwrap().inbox.push(env_bytes(0));
        s0.lock().unwrap().inbox.push(env_bytes(1));
        assert_eq!(node.poll(), 1);
        assert_eq!(node.drain_delivered().len(), 1);
    }

    #[test]
    fn test_poll_drops_at_hop_limit() {
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        let (b1, s1) = mock(BackendKind::I2p, 2);
        node.add_backend(Box::new(b0));
        node.add_backend(Box::new(b1));
        node.start().unwrap();
        s0.lock().unwrap().inbox.push(env_bytes(MAX_HOP_COUNT));
        assert_eq!(node.poll(), 1);
        assert_eq!(node.drain_delivered().len(), 1);
        assert!(s1.lock().unwrap().outbox.is_empty());
    }

    #[test]
    fn test_poll_ignores_garbage_payloads() {
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        node.add_backend(Box::new(b0));
        node.start().unwrap();
        s0.lock().unwrap().inbox.push(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(node.poll(), 0);
        assert!(node.drain_delivered().is_empty());
    }

    #[test]
    fn test_status_json_shape() {
        let mut node = RelayNode::new();
        node.add_backend(Box::new(mock(BackendKind::Reticulum, 2).0));
        node.start().unwrap();
        node.publish("e1", 1, "a", 0, b"p".to_vec()).unwrap();
        let status: serde_json::Value = serde_json::from_str(&node.status()).unwrap();
        assert_eq!(status["running"], true);
        assert_eq!(status["peers"]["reticulum"], 2);
        assert_eq!(status["published"], 1);
    }

    #[test]
    fn test_start_fails_with_no_backend() {
        let mut node = RelayNode::new();
        assert!(node.start().is_err());
        assert!(!node.running());
    }
}
