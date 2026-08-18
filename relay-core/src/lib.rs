//! Device-as-relay mesh core.
//!
//! Every device runs a relay node that exchanges signed events with peers
//! over mesh transports (Reticulum, I2P, Freenet) using a custom binary
//! envelope — no nostr relay protocol on the wire. Events are verified at
//! the ingestion boundary (flutter-bridge) with the existing event.verify
//! machinery; this crate only validates envelope structure, applies
//! hostile-input caps, dedups by event id, and re-broadcasts with a hop cap
//! (flood-style store-and-forward, torrent-like).

pub mod backends;
pub mod envelope;
pub mod relay;
