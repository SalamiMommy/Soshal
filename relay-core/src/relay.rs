//! The relay node: store-and-forward gossip engine over mesh backends.
//!
//! Flood-style: published events are broadcast at hop 0; inbound envelopes
//! are deduped by payload digest and re-broadcast once at hop+1 (ceiling
//! MAX_HOP_COUNT). Delivered payloads are queued for the app to drain.

use std::collections::VecDeque;

use sha2::{Digest, Sha256};

use crate::backends::{BackendKind, MeshBackend};
use crate::envelope::MeshEnvelope;
use soshal_common_core::bounded::BoundedSet;

/// Recent-event ring buffer size (re-broadcastable history).
const RECENT_CAPACITY: usize = 10_000;
/// Seen-id dedup set cap.
const SEEN_CAPACITY: usize = 100_000;
/// App-delivery queue cap.
const DELIVERED_CAPACITY: usize = 4_096;

/// Unforgeable dedup key for an envelope: sha256 of its payload bytes.
fn payload_digest(payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    format!("{:x}", hasher.finalize())
}

/// Payloads are signed event JSON. Reject anything that does not parse as an
/// event with a valid Schnorr signature: a mesh peer must not be able to
/// inject garbage that fans out to every backend at hop+1 (flood
/// amplification). Event JSON is self-authenticating — no key material
/// needed to verify.
fn valid_event_payload(payload: &[u8]) -> bool {
    let Ok(json) = std::str::from_utf8(payload) else {
        return false;
    };
    match nostr::event::Event::from_json(json) {
        Ok(event) => soshal_nostr_core::models::verify_event(&event),
        Err(_) => false,
    }
}

/// Flood gossip relay node.
pub struct RelayNode {
    backends: Vec<Box<dyn MeshBackend>>,
    recent: VecDeque<MeshEnvelope>,
    seen: BoundedSet<String>,
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
            seen: BoundedSet::new(SEEN_CAPACITY),
            delivered: VecDeque::new(),
            published: 0,
            received: 0,
            running: false,
        }
    }

    pub fn add_backend(&mut self, backend: Box<dyn MeshBackend>) {
        self.backends.push(backend);
    }

    /// Builds a node with backends matching a transport mode: `Default` gets
    /// all three mesh backends; "only" modes pin the single backend. `Nostr`
    /// yields no backends (mesh is off — `start()` fails cleanly).
    pub fn new_with_transport(
        mode: soshal_network_core::transport::TransportMode,
        pubkey: &str,
    ) -> Self {
        use soshal_network_core::transport::TransportMode as M;
        let pubkey_clean = pubkey.trim().to_ascii_lowercase();
        let mut node = Self::new();
        match mode {
            M::Default => {
                node.add_backend(Box::new(crate::backends::reticulum::ReticulumBackend::new(
                    pubkey_clean.clone(),
                )));
                node.add_backend(Box::new(crate::backends::i2p::I2pBackend::new()));
                node.add_backend(Box::new(crate::backends::freenet::FreenetBackend::new()));
            }
            M::Reticulum => {
                node.add_backend(Box::new(crate::backends::reticulum::ReticulumBackend::new(
                    pubkey_clean,
                )));
            }
            M::Freenet => {
                node.add_backend(Box::new(crate::backends::freenet::FreenetBackend::new()));
            }
            M::I2p => {
                node.add_backend(Box::new(crate::backends::i2p::I2pBackend::new()));
            }
            M::Nostr => {}
        }
        node
    }

    /// Kinds of backends that are currently running (drives transport
    /// resolution: a running mesh backend counts as its transport being up).
    pub fn running_backends(&self) -> Vec<BackendKind> {
        self.backends
            .iter()
            .filter(|b| b.running())
            .map(|b| b.kind())
            .collect()
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
        let event_id_clean = event_id.trim().to_ascii_lowercase();
        let author_clean = author.trim().to_ascii_lowercase();
        if event_id_clean.is_empty() || author_clean.is_empty() {
            return Err("event id or author cannot be empty".to_string());
        }
        if event_id_clean.len() > 128 || author_clean.len() > 128 {
            return Err("event id or author exceeds 128-byte cap".to_string());
        }
        let env = MeshEnvelope::new(event_id_clean, kind, author_clean, created_at, payload);
        self.note_seen(payload_digest(&env.payload));
        self.recent.push_back(env.clone());
        if self.recent.len() > RECENT_CAPACITY {
            self.recent.pop_front();
        }
        let mut total = 0usize;
        for backend in self.backends.iter_mut() {
            if backend.running() {
                total += backend
                    .broadcast(env.to_bytes().unwrap_or_default())
                    .unwrap_or(0);
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
        // Frame + signature verification are pure CPU; batch them across all
        // backends and run the Schnorr checks in parallel (each verify is a
        // secp256k1 fixpoint ~0.1 ms). Dedup/queue/re-broadcast stay strictly
        // sequential so `seen`/`recent`/`delivered` order is deterministic.
        let mut jobs: Vec<(usize, MeshEnvelope)> = Vec::new();
        'inbound_loop: for (idx, payloads) in inbound {
            for payload in payloads {
                if jobs.len() >= DELIVERED_CAPACITY {
                    break 'inbound_loop;
                }
                let Some(env) = MeshEnvelope::from_bytes(&payload) else {
                    continue;
                };
                jobs.push((idx, env));
            }
        }
        let verified: Vec<bool> = if jobs.len() >= 4 {
            use rayon::prelude::*;
            jobs.par_iter()
                .map(|(_, env)| valid_event_payload(&env.payload))
                .collect()
        } else {
            jobs.iter()
                .map(|(_, env)| valid_event_payload(&env.payload))
                .collect()
        };

        let mut new_count = 0usize;
        for ((idx, env), ok) in jobs.into_iter().zip(verified) {
            // Verify before dedup/delivery/re-broadcast: forged or garbage
            // payloads die at the first relay hop.
            if !ok {
                continue;
            }
            // Dedup on the payload digest, NOT the envelope's event_id
            // header: the id claim is unverified at the relay layer, so
            // keying on it lets a forged envelope reuse a legit event's
            // id and suppress delivery/re-broadcast of the real event.
            // Identical payloads still dedup once.
            let digest = payload_digest(&env.payload);
            if self.seen.contains(&digest) {
                continue;
            }
            self.note_seen(digest);
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
        new_count
    }

    /// Drains payloads queued for the app.
    pub fn drain_delivered(&mut self) -> Vec<Vec<u8>> {
        self.delivered.drain(..).collect()
    }

    /// Payloads from the re-broadcastable recent ring, newest first, capped
    /// at `limit`. Backs the mesh fetch path: flood gossip keeps no per-
    /// consumer history, so this is the mesh's queryable event window.
    pub fn recent_payloads(&self, limit: usize) -> Vec<Vec<u8>> {
        self.recent
            .iter()
            .rev()
            .take(limit)
            .map(|env| env.payload.clone())
            .collect()
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
        let Some(bytes) = env.to_bytes().ok() else {
            return;
        };
        for (idx, backend) in self.backends.iter_mut().enumerate() {
            if idx != source_idx && backend.running() {
                let _ = backend.broadcast(bytes.clone());
            }
        }
    }

    fn note_seen(&mut self, id: String) {
        self.seen.insert(id);
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
    use nostr::event::FinalizeEvent;
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
        let keys = nostr::key::Keys::generate();
        let event = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, "hello")
            .finalize(&keys)
            .unwrap();
        let payload = serde_json::to_vec(&event).unwrap();
        let mut env = MeshEnvelope::new(
            event.id.to_hex(),
            1,
            event.pubkey.to_hex(),
            event.created_at.as_secs(),
            payload,
        );
        env.hop_count = hop;
        env.to_bytes().unwrap()
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
        let delivered = node.drain_delivered();
        assert_eq!(delivered.len(), 1);
        assert!(valid_event_payload(&delivered[0]));
        let outbox = s1.lock().unwrap().outbox.clone();
        assert_eq!(outbox.len(), 1);
        let env = MeshEnvelope::from_bytes(&outbox[0]).unwrap();
        assert_eq!(env.hop_count, 1);
    }

    #[test]
    fn test_poll_dedups_identical_payloads() {
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        node.add_backend(Box::new(b0));
        node.start().unwrap();
        let bytes = env_bytes(0);
        s0.lock().unwrap().inbox.push(bytes.clone());
        s0.lock().unwrap().inbox.push(bytes);
        assert_eq!(node.poll(), 1);
        assert_eq!(node.drain_delivered().len(), 1);
    }

    #[test]
    fn test_poll_does_not_dedup_forged_same_id_different_payload() {
        // Forged envelope reusing a legit event_id must not suppress the
        // real event: dedup keys on the payload digest, not the id header.
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        node.add_backend(Box::new(b0));
        node.start().unwrap();
        let keys = nostr::key::Keys::generate();
        let event = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, "real")
            .finalize(&keys)
            .unwrap();
        let payload = serde_json::to_vec(&event).unwrap();
        let legit = MeshEnvelope::new(
            event.id.to_hex(),
            1,
            event.pubkey.to_hex(),
            event.created_at.as_secs(),
            payload,
        );
        let mut forged = legit.clone();
        forged.payload = b"forged payload".to_vec();
        s0.lock().unwrap().inbox.push(legit.to_bytes().unwrap());
        s0.lock().unwrap().inbox.push(forged.to_bytes().unwrap());
        // Only the signed event is accepted; the forged one is dropped.
        assert_eq!(node.poll(), 1);
        assert_eq!(node.drain_delivered().len(), 1);
    }

    #[test]
    fn test_poll_rejects_invalid_signature() {
        let mut node = RelayNode::new();
        let (b0, s0) = mock(BackendKind::Reticulum, 2);
        node.add_backend(Box::new(b0));
        node.start().unwrap();
        let keys = nostr::key::Keys::generate();
        let event = nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, "real")
            .finalize(&keys)
            .unwrap();
        let mut json: serde_json::Value = serde_json::to_value(&event).unwrap();
        json["sig"] = serde_json::Value::String("00".repeat(64));
        let tampered = serde_json::to_vec(&json).unwrap();
        let mut env = MeshEnvelope::new(
            event.id.to_hex(),
            1,
            event.pubkey.to_hex(),
            event.created_at.as_secs(),
            tampered,
        );
        env.hop_count = 0;
        s0.lock().unwrap().inbox.push(env.to_bytes().unwrap());
        assert_eq!(node.poll(), 0, "bad-signature payload must be dropped");
        assert!(node.drain_delivered().is_empty());
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
    fn test_publish_rejects_empty_id_and_author() {
        let mut node = RelayNode::new();
        node.add_backend(Box::new(mock(BackendKind::Reticulum, 1).0));
        node.start().unwrap();
        assert!(node.publish("", 1, "a", 0, b"p".to_vec()).is_err());
        assert!(node.publish("e1", 1, "", 0, b"p".to_vec()).is_err());
    }

    #[test]
    fn test_start_fails_with_no_backend() {
        let mut node = RelayNode::new();
        assert!(node.start().is_err());
        assert!(!node.running());
    }

    #[test]
    fn test_recent_payloads_newest_first_capped() {
        let mut node = RelayNode::new();
        let (b0, _s0) = mock(BackendKind::Reticulum, 0);
        node.add_backend(Box::new(b0));
        node.start().unwrap();
        let keys = nostr::key::Keys::generate();
        for i in 0..3 {
            let event =
                nostr::event::EventBuilder::new(nostr::event::Kind::TextNote, format!("msg {i}"))
                    .finalize(&keys)
                    .unwrap();
            node.publish(
                &event.id.to_hex(),
                1,
                &event.pubkey.to_hex(),
                event.created_at.as_secs(),
                serde_json::to_vec(&event).unwrap(),
            )
            .unwrap();
        }
        let payloads = node.recent_payloads(2);
        assert_eq!(payloads.len(), 2);
        assert!(String::from_utf8_lossy(&payloads[0]).contains("msg 2"));
        assert!(String::from_utf8_lossy(&payloads[1]).contains("msg 1"));
    }

    #[test]
    fn test_new_with_transport_nostr_has_no_backends() {
        let mut node = RelayNode::new_with_transport(
            soshal_network_core::transport::TransportMode::Nostr,
            "pk",
        );
        assert!(node.running_backends().is_empty());
        assert!(node.start().is_err());
        assert!(!node.running());
    }

    #[test]
    fn test_new_with_transport_only_mode_pins_backend() {
        let mut node = RelayNode::new_with_transport(
            soshal_network_core::transport::TransportMode::Reticulum,
            "pk_reticulum_only",
        );
        let ok = node.start().unwrap();
        assert_eq!(ok, 1);
        assert_eq!(node.running_backends(), vec![BackendKind::Reticulum]);
        node.stop();
        assert!(node.running_backends().is_empty());
    }

    #[test]
    fn test_new_with_transport_default_runs_reticulum_without_daemons() {
        // No i2pd/freenet in the test env: Default still starts via the
        // in-process Reticulum node registry; i2p/freenet backends fail.
        let mut node = RelayNode::new_with_transport(
            soshal_network_core::transport::TransportMode::Default,
            "pk_default",
        );
        let ok = node.start().unwrap();
        assert!(ok >= 1);
        assert!(node.running_backends().contains(&BackendKind::Reticulum));
        node.stop();
    }

    #[test]
    fn test_publish_normalizes_casing() {
        let mut node = RelayNode::new();
        node.add_backend(Box::new(mock(BackendKind::Reticulum, 1).0));
        node.start().unwrap();

        let peers = node
            .publish(
                "UPPERCASE_ID",
                1,
                "UPPERCASE_AUTHOR",
                123,
                b"test payload".to_vec(),
            )
            .unwrap();
        assert_eq!(peers, 1);
        let recent = node.recent_payloads(1);
        assert_eq!(recent.len(), 1);
        assert_eq!(node.recent[0].event_id, "uppercase_id");
        assert_eq!(node.recent[0].author, "uppercase_author");
    }
}
