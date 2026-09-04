//! Background relay sync engine.
//!
//! Dedicated thread + tokio runtime that owns its own `nostr-sdk` client
//! (WebSocket keepalive, reconnects, backpressure all handled by nostr-sdk
//! inside Rust). Incoming verified events are written to the local SQLite
//! cache (`db-core`) and forwarded to the app as [`SyncUpdate`] values over a
//! tokio mpsc channel; the bridge layer relays them to Flutter via a
//! StreamSink.
//!
//! Key material never enters this crate: DM decryption happens in the bridge
//! signer module. The engine only forwards encrypted payloads addressed to
//! the local pubkey.

pub mod delta;
pub mod engine;
pub mod epoch_gc;
pub mod gossip;
pub mod ingest;
pub mod outbox;
pub mod prolly_sync;
pub mod prolly_tree;
pub mod revert;
pub mod tx;
pub mod zk_rollup;

use serde::{Deserialize, Serialize};

/// Settings keys holding the per-kind sync watermark (`max created_at` seen).
pub const WM_FEED: &str = "sync_watermark_k1";
pub const WM_DM: &str = "sync_watermark_k4";
pub const WM_META: &str = "sync_watermark_k0";

/// An event delivered to the app after being verified + cached locally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncUpdate {
    /// A cached feed post (kind 1/etc). Content ready for display.
    Feed {
        id: String,
        pubkey: String,
        content: String,
        created_at: u64,
        kind: u64,
    },
    /// An incoming kind-4 payload addressed to us. `content` is still NIP-44
    /// encrypted; the bridge decrypts before persisting/displaying.
    Dm {
        id: String,
        sender: String,
        recipient: String,
        content: String,
        created_at: u64,
    },
    /// A cached reaction to a post.
    Reaction {
        id: String,
        event_id: String,
        pubkey: String,
        content: String,
        created_at: u64,
    },
    /// A cached profile (kind 0) was upserted.
    Profile { pubkey: String },
}
