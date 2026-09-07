// Re-export json utilities from common-core
pub use soshal_common_core::json_util::{json_in, json_out};

pub mod blob_grab;
pub mod discovery;
pub mod ebpf;
pub mod eigentrust;
pub mod heuristics;
pub mod http3_client;
pub mod lan;
pub mod lan_transport;
pub mod mdns;
pub mod multi_bearer;
pub mod nat;
pub mod network;
pub mod outbox_ranking;
pub mod peer_filter;
pub mod plumtree;
pub mod power;
pub mod privacy;
pub mod quic;
pub mod quic_migration;
pub mod rate_limit;
pub mod relay_health;
pub mod skademlia;
pub mod swarm;
pub mod transport;

// Mesh & alternative transport family moved to `soshal-mesh-core`; re-exported
// here so existing `network_core::<mesh_module>` paths keep resolving for
// flutter-bridge / relay-core / sync-core callers.
pub use soshal_mesh_core::{
    ble, freenet_cache_router, freenet_contract, freenet_opennet, freenet_websocket, i2p_sam,
    p2p_frame, pqc_link, reticulum, wifi_direct,
};
