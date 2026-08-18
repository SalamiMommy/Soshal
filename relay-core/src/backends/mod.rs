//! Mesh transport backends.
//!
//! Each backend adapts one physical mesh transport to the relay node's
//! needs: broadcast payloads to peers, surface inbound payloads, report
//! peer counts.

pub mod freenet;
pub mod i2p;
pub mod reticulum;

/// Transport identity for status reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Reticulum,
    I2p,
    Freenet,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reticulum => "reticulum",
            Self::I2p => "i2p",
            Self::Freenet => "freenet",
        }
    }
}

/// A mesh transport adapter.
pub trait MeshBackend: Send {
    fn kind(&self) -> BackendKind;

    /// Starts the underlying transport (idempotent; Err on failure).
    fn start(&mut self) -> Result<(), String>;

    /// Stops the underlying transport (idempotent).
    fn stop(&mut self);

    fn running(&self) -> bool;

    /// Broadcasts a payload to all known peers. Returns the peer count the
    /// payload was handed to (0 when not running or no peers).
    fn broadcast(&mut self, payload: Vec<u8>) -> Result<usize, String>;

    /// Drains inbound payloads received since the last call.
    fn recv(&mut self) -> Vec<Vec<u8>>;

    /// Human-readable peer identifiers (socket addrs, I2P destination hashes).
    fn peers(&self) -> Vec<String>;

    fn peers_count(&self) -> usize;

    /// Downcast helper for backend-specific operations.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
