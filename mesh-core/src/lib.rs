//! Mesh & alternative transports: freenet, I2P, Reticulum, BLE, WiFi-Direct,
//! plus the shared PQC link encryption and P2P frame primitives they rely on.
//!
//! Split out of `soshal-network-core` so the exotic/mesh transport family
//! compiles and evolves independently of the core P2P bulk-transport stack
//! (lan/quic/swarm).

pub mod ble;
pub mod freenet_cache_router;
pub mod freenet_contract;
pub mod freenet_opennet;
pub mod freenet_websocket;
pub mod i2p_sam;
pub mod p2p_frame;
pub mod pqc_link;
pub mod reticulum;
pub mod wifi_direct;
