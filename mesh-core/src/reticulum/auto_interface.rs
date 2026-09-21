//! Reticulum AutoInterface for peer discovery over local networks.
//!
//! AutoInterface enables automatic peer discovery over Ethernet and WiFi
//! using IPv6 link-local multicast and UDP. This implementation provides
//! compatibility with Reticulum's AutoInterface behavior.

use super::address::ReticulumAddress;
use super::interface::{ReticulumInterfaceKind, ReticulumInterfaceStatus};
use serde::{Deserialize, Serialize};
use std::net::{Ipv6Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const AUTO_DISCOVERY_PORT: u16 = 4242;
const AUTO_DISCOVERY_GROUP: &str = "ff02::1"; // IPv6 all-nodes multicast
const BEACON_INTERVAL_MS: u64 = 5000; // 5 seconds
const BEACON_MAGIC: &[u8] = b"RN\0"; // Reticulum magic bytes
const BEACON_VERSION: u32 = 1; // Metadata interface version

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoInterfaceConfig {
    pub enabled: bool,
    pub bind_port: u16,
    pub discovery_interval_ms: u64,
}

impl Default for AutoInterfaceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_port: AUTO_DISCOVERY_PORT,
            discovery_interval_ms: BEACON_INTERVAL_MS,
        }
    }
}

pub struct AutoInterface {
    config: AutoInterfaceConfig,
    socket: Option<Arc<UdpSocket>>,
    running: Arc<AtomicBool>,
    discovery_thread: Option<thread::JoinHandle<()>>,
    local_destination: ReticulumAddress,
}

impl AutoInterface {
    pub fn new(local_destination: ReticulumAddress, config: AutoInterfaceConfig) -> Self {
        Self {
            config,
            socket: None,
            running: Arc::new(AtomicBool::new(false)),
            discovery_thread: None,
            local_destination,
        }
    }

    /// Starts the AutoInterface for peer discovery
    pub fn start(&mut self) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(()); // Disabled, no-op
        }

        let bind_addr = format!("[::]:{}", self.config.bind_port);
        let socket =
            UdpSocket::bind(&bind_addr).map_err(|e| format!("AutoInterface bind failed: {e}"))?;

        socket
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking failed: {e}"))?;

        // Join IPv6 multicast group
        if let Ok(multi_addr) = AUTO_DISCOVERY_GROUP.parse::<Ipv6Addr>() {
            socket
                .join_multicast_v6(&multi_addr, 0)
                .map_err(|e| format!("multicast join failed: {e}"))?;
        }

        let socket_arc = Arc::new(socket);
        let running = self.running.clone();
        let local_dest = self.local_destination;
        let interval = Duration::from_millis(self.config.discovery_interval_ms);

        // Set running BEFORE spawning (mirrors TcpServerInterface::start):
        // otherwise the thread can observe `false` and exit immediately,
        // leaving a dead interface that can't be restarted (bind conflict).
        self.running.store(true, Ordering::Relaxed);
        let socket_clone = socket_arc.clone();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 2048];
            let mut last_beacon = std::time::Instant::now();

            while running.load(Ordering::Relaxed) {
                // Send periodic beacons
                if last_beacon.elapsed() >= interval {
                    let beacon = Self::create_beacon(&local_dest);
                    let _ = socket_clone.send_to(
                        &beacon,
                        format!("{}:{}", AUTO_DISCOVERY_GROUP, AUTO_DISCOVERY_PORT),
                    );
                    last_beacon = std::time::Instant::now();
                }

                // Receive beacons from peers
                match socket_clone.recv_from(&mut buf) {
                    Ok((len, src)) => {
                        if len > BEACON_MAGIC.len() && &buf[..BEACON_MAGIC.len()] == BEACON_MAGIC {
                            if let Some(peer_dest) = Self::parse_beacon(&buf[..len]) {
                                // In a full implementation, this would notify the ReticulumNode
                                // of a discovered peer for path table updates
                                println!("Discovered Reticulum peer: {} from {}", peer_dest, src);
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                }
            }
        });

        self.socket = Some(socket_arc);
        self.discovery_thread = Some(handle);

        Ok(())
    }

    /// Stops the AutoInterface
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.discovery_thread.take() {
            let _ = handle.join();
        }
        self.socket = None;
    }

    /// Creates a discovery beacon packet
    fn create_beacon(destination: &ReticulumAddress) -> Vec<u8> {
        let mut beacon = Vec::new();
        beacon.extend_from_slice(BEACON_MAGIC);
        beacon.extend_from_slice(destination.0.as_ref());
        // Metadata: interface version (u32 LE) + beacon interval seconds (u32 LE)
        beacon.extend_from_slice(&BEACON_VERSION.to_le_bytes());
        beacon.extend_from_slice(&(BEACON_INTERVAL_MS as u32 / 1000).to_le_bytes());
        beacon
    }

    /// Parses a discovery beacon packet
    fn parse_beacon(data: &[u8]) -> Option<ReticulumAddress> {
        let magic_len = BEACON_MAGIC.len();
        if data.len() < magic_len + 16 {
            return None;
        }

        if &data[..magic_len] != BEACON_MAGIC {
            return None;
        }

        let mut addr_bytes = [0u8; 16];
        addr_bytes.copy_from_slice(&data[magic_len..magic_len + 16]);

        Some(ReticulumAddress::from_bytes(addr_bytes))
    }

    /// Gets the interface status
    pub fn get_status(&self) -> ReticulumInterfaceStatus {
        ReticulumInterfaceStatus {
            name: "AutoInterface (Multicast)".to_string(),
            kind: ReticulumInterfaceKind::UdpMulticast,
            bind_address: format!("[::]:{}", self.config.bind_port),
            active: self.running.load(Ordering::Relaxed),
            rx_packets: 0, // Would need to track actual packet counts
            tx_packets: 0,
        }
    }
}

impl Drop for AutoInterface {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beacon_creation() {
        let dest = ReticulumAddress::from_pubkey("test_pubkey");
        let beacon = AutoInterface::create_beacon(&dest);

        assert_eq!(&beacon[..3], BEACON_MAGIC);
        assert_eq!(beacon.len(), BEACON_MAGIC.len() + 16 + 8);
    }

    #[test]
    fn test_beacon_parsing() {
        let dest = ReticulumAddress::from_pubkey("test_pubkey");
        let beacon = AutoInterface::create_beacon(&dest);

        let parsed = AutoInterface::parse_beacon(&beacon).unwrap();
        assert_eq!(parsed, dest);
    }

    #[test]
    fn test_invalid_beacon_rejected() {
        let invalid = vec![0u8; 10];
        assert!(AutoInterface::parse_beacon(&invalid).is_none());
    }

    #[test]
    fn test_auto_interface_config_default() {
        let config = AutoInterfaceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.bind_port, AUTO_DISCOVERY_PORT);
        assert_eq!(config.discovery_interval_ms, BEACON_INTERVAL_MS);
    }

    #[test]
    fn test_disabled_start_noop_and_status_shape() {
        let dest = ReticulumAddress::from_pubkey("test_pubkey");
        let config = AutoInterfaceConfig {
            enabled: false,
            bind_port: 9999,
            discovery_interval_ms: 100,
        };
        let mut iface = AutoInterface::new(dest, config);

        assert!(iface.start().is_ok());
        assert!(iface.socket.is_none());
        assert!(!iface.running.load(Ordering::Relaxed));

        let status = iface.get_status();
        assert_eq!(status.name, "AutoInterface (Multicast)");
        assert!(matches!(status.kind, ReticulumInterfaceKind::UdpMulticast));
        assert_eq!(status.bind_address, "[::]:9999");
        assert!(!status.active);

        // Fresh disabled instance: still no-op
        let fresh = AutoInterface::new(
            ReticulumAddress::from_pubkey("other_pubkey"),
            AutoInterfaceConfig {
                enabled: false,
                bind_port: 4242,
                discovery_interval_ms: 5000,
            },
        );
        assert!(!fresh.running.load(Ordering::Relaxed));
        assert!(!fresh.get_status().active);
    }

    #[test]
    fn test_beacon_bad_magic_exact_boundary_and_le_metadata() {
        // Bad magic prefix -> None
        let mut bad = Vec::new();
        bad.extend_from_slice(b"XX\0");
        bad.extend_from_slice(&[0u8; 16]);
        assert!(AutoInterface::parse_beacon(&bad).is_none());

        // Exact-length boundary (len == magic + 16) -> parses
        let mut exact = Vec::new();
        exact.extend_from_slice(BEACON_MAGIC);
        exact.extend_from_slice(&[7u8; 16]);
        let parsed = AutoInterface::parse_beacon(&exact).unwrap();
        assert_eq!(parsed.0, [7u8; 16]);

        // create_beacon metadata is little-endian: version then interval seconds
        let dest = ReticulumAddress::from_pubkey("test_pubkey");
        let beacon = AutoInterface::create_beacon(&dest);
        let magic_len = BEACON_MAGIC.len();
        assert_eq!(
            &beacon[magic_len + 16..magic_len + 20],
            BEACON_VERSION.to_le_bytes()
        );
        assert_eq!(
            &beacon[magic_len + 20..magic_len + 24],
            (BEACON_INTERVAL_MS as u32 / 1000).to_le_bytes()
        );
    }
}
