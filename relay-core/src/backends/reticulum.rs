//! Reticulum mesh backend: wraps `ReticulumNode` Data broadcast delivery.

use crate::backends::{BackendKind, MeshBackend};

pub struct ReticulumBackend {
    pubkey: String,
    node: Option<
        std::sync::Arc<std::sync::Mutex<soshal_network_core::reticulum::transport::ReticulumNode>>,
    >,
    started: bool,
}

impl ReticulumBackend {
    pub fn new(pubkey: String) -> Self {
        Self {
            pubkey,
            node: None,
            started: false,
        }
    }
}

impl MeshBackend for ReticulumBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Reticulum
    }
    fn start(&mut self) -> Result<(), String> {
        let node = soshal_network_core::reticulum::node_for(&self.pubkey)?;
        self.node = Some(node);
        self.started = true;
        Ok(())
    }
    fn stop(&mut self) {
        self.started = false;
        self.node = None;
    }
    fn running(&self) -> bool {
        self.started
    }
    fn broadcast(&mut self, payload: Vec<u8>) -> Result<usize, String> {
        let node = self.node.as_ref().ok_or("reticulum backend not started")?;
        let guard = node.lock().unwrap_or_else(|e| e.into_inner());
        Ok(guard.broadcast_data(payload))
    }
    fn recv(&mut self) -> Vec<Vec<u8>> {
        let Some(node) = self.node.as_ref() else {
            return Vec::new();
        };
        let guard = node.lock().unwrap_or_else(|e| e.into_inner());
        guard.drain_delivered()
    }
    fn peers(&self) -> Vec<String> {
        let Some(node) = self.node.as_ref() else {
            return Vec::new();
        };
        let guard = node.lock().unwrap_or_else(|e| e.into_inner());
        guard.known_peers().iter().map(|a| a.to_string()).collect()
    }
    fn peers_count(&self) -> usize {
        let Some(node) = self.node.as_ref() else {
            return 0;
        };
        let guard = node.lock().unwrap_or_else(|e| e.into_inner());
        guard.peers_count()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
