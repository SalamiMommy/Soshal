//! Reticulum mesh network node coordinator and social transport adapter.

use super::address::ReticulumAddress;
use super::auto_interface::{AutoInterface, AutoInterfaceConfig};
use super::interface::{ReticulumInterfaceKind, ReticulumInterfaceStatus};
use super::link::LinkManager;
use super::packet::{ReticulumPacket, ReticulumPacketType, MAX_HOPS};
use super::routing::PathTable;
use super::tcp_interface::{TcpInterfaceConfig, TcpServerInterface};
use serde::{Deserialize, Serialize};
use soshal_common_core::format::now_secs;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{SocketAddr, UdpSocket};

use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReticulumNodeStatus {
    pub running: bool,
    pub destination_hash: String,
    pub active_routes: usize,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub interfaces: Vec<ReticulumInterfaceStatus>,
}

pub struct ReticulumNode {
    pub destination: ReticulumAddress,
    pub path_table: Arc<Mutex<PathTable>>,
    pub interfaces: Arc<Mutex<Vec<ReticulumInterfaceStatus>>>,
    pub rx_count: Arc<Mutex<u64>>,
    pub tx_count: Arc<Mutex<u64>>,
    pub running: Arc<Mutex<bool>>,
    pub link_manager: Arc<LinkManager>,
    pub peers: Arc<Mutex<HashSet<SocketAddr>>>,
    pub delivered: Arc<Mutex<VecDeque<Vec<u8>>>>,
    udp_socket: Option<Arc<UdpSocket>>,
    transport_thread: Option<thread::JoinHandle<()>>,
    auto_interface: Option<AutoInterface>,
    tcp_server: Option<TcpServerInterface>,
}

impl ReticulumNode {
    pub fn new(pubkey: &str) -> Self {
        let destination = ReticulumAddress::from_pubkey(pubkey);
        let default_iface = ReticulumInterfaceStatus::new_udp("0.0.0.0:4242", true);

        Self {
            destination,
            path_table: Arc::new(Mutex::new(PathTable::new())),
            interfaces: Arc::new(Mutex::new(vec![default_iface])),
            rx_count: Arc::new(Mutex::new(0)),
            tx_count: Arc::new(Mutex::new(0)),
            running: Arc::new(Mutex::new(true)),
            link_manager: Arc::new(LinkManager::new()),
            peers: Arc::new(Mutex::new(HashSet::new())),
            delivered: Arc::new(Mutex::new(VecDeque::new())),
            udp_socket: None,
            transport_thread: None,
            auto_interface: None,
            tcp_server: None,
        }
    }

    /// Starts the AutoInterface for peer discovery
    pub fn start_auto_interface(&mut self, config: AutoInterfaceConfig) -> Result<(), String> {
        let mut auto = AutoInterface::new(self.destination, config);
        auto.start()?;
        self.auto_interface = Some(auto);

        // Update interface list
        let mut iface_guard = self.interfaces.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref auto) = self.auto_interface {
            iface_guard.push(auto.get_status());
        }

        Ok(())
    }

    /// Starts the TCP server interface
    pub fn start_tcp_server(&mut self, config: TcpInterfaceConfig) -> Result<(), String> {
        let mut tcp = TcpServerInterface::new(self.destination, config);
        tcp.start()?;
        self.tcp_server = Some(tcp);

        // Update interface list
        let mut iface_guard = self.interfaces.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref tcp) = self.tcp_server {
            iface_guard.push(tcp.get_status());
        }

        Ok(())
    }

    /// Starts the UDP transport layer for Reticulum communication
    pub fn start_udp_transport(&mut self, bind_addr: &str) -> Result<(), String> {
        let socket = UdpSocket::bind(bind_addr).map_err(|e| format!("UDP bind failed: {e}"))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking failed: {e}"))?;

        let udp_socket = Arc::new(socket);
        let running = self.running.clone();
        let path_table = self.path_table.clone();
        let interfaces = self.interfaces.clone();
        let rx_count = self.rx_count.clone();
        let _tx_count = self.tx_count.clone();
        let destination = self.destination;
        let link_manager = self.link_manager.clone();
        let peers = self.peers.clone();
        let delivered = self.delivered.clone();

        let udp_clone = udp_socket.clone();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 2048];
            let mut ticks: u64 = 0;

            while *running.lock().unwrap_or_else(|e| e.into_inner()) {
                ticks = ticks.wrapping_add(1);
                // Periodic maintenance: prune expired routes every ~1000
                // iterations (~10 s when idle) so attacker-filled tables
                // cannot grow without bound.
                if ticks.is_multiple_of(1000) {
                    let now = now_secs() as u64;
                    path_table
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .prune_expired(now);
                }
                match udp_clone.recv_from(&mut buf) {
                    Ok((len, src_addr)) => {
                        if let Ok(packet_data) = super::slip::slip_decode(&buf[..len]) {
                            if let Ok(pkt) = ReticulumPacket::from_bytes(&packet_data) {
                                let mut rx_guard =
                                    rx_count.lock().unwrap_or_else(|e| e.into_inner());
                                *rx_guard += 1;

                                peers
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .insert(src_addr);

                                if pkt.packet_type == ReticulumPacketType::Data
                                    && pkt.payload.len() <= 262144
                                {
                                    if pkt.destination.is_broadcast()
                                        || pkt.destination == destination
                                    {
                                        let mut dq =
                                            delivered.lock().unwrap_or_else(|e| e.into_inner());
                                        if dq.len() >= 4096 {
                                            dq.pop_front();
                                        }
                                        dq.push_back(pkt.payload.clone());
                                    }
                                    if pkt.hops < MAX_HOPS {
                                        let mut fwd = pkt.clone();
                                        fwd.increment_hops_in_place();
                                        let known: Vec<SocketAddr> = peers
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner())
                                            .iter()
                                            .copied()
                                            .filter(|p| *p != src_addr)
                                            .collect();
                                        let fwd_bytes = super::slip::slip_encode(&fwd.to_bytes());
                                        for peer in known {
                                            let _ = udp_clone.send_to(&fwd_bytes, peer);
                                        }
                                    }
                                }

                                let now_secs = now_secs() as u64;

                                if pkt.packet_type == ReticulumPacketType::Announce {
                                    let mut path_guard =
                                        path_table.lock().unwrap_or_else(|e| e.into_inner());
                                    path_guard.update_route(
                                        pkt.destination,
                                        pkt.destination,
                                        pkt.hops,
                                        now_secs,
                                    );
                                } else if pkt.packet_type == ReticulumPacketType::LinkRequest {
                                    let _ = link_manager
                                        .handle_link_request(pkt.destination, &pkt.payload);
                                } else if pkt.packet_type == ReticulumPacketType::Proof {
                                    let _ = link_manager
                                        .handle_link_proof(pkt.destination, &pkt.payload);
                                }

                                link_manager.update_activity(&pkt.destination);

                                // Update interface stats
                                let mut iface_guard =
                                    interfaces.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(iface) = iface_guard
                                    .iter_mut()
                                    .find(|i| i.kind == ReticulumInterfaceKind::UdpUnicast)
                                {
                                    iface.rx_packets += 1;
                                }
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                }
            }
        });

        self.udp_socket = Some(udp_socket);
        self.transport_thread = Some(handle);

        // Update interface binding address
        let mut iface_guard = self.interfaces.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(iface) = iface_guard
            .iter_mut()
            .find(|i| i.kind == ReticulumInterfaceKind::UdpUnicast)
        {
            iface.bind_address = bind_addr.to_string();
        }

        Ok(())
    }

    /// Sends a Reticulum packet via UDP transport
    pub fn send_packet(
        &self,
        dest_addr: SocketAddr,
        packet: &ReticulumPacket,
    ) -> Result<(), String> {
        let socket = self
            .udp_socket
            .as_ref()
            .ok_or("UDP transport not started")?;

        let packet_bytes = packet.to_bytes();
        let slip_encoded = super::slip::slip_encode(&packet_bytes);

        socket
            .send_to(&slip_encoded, dest_addr)
            .map_err(|e| format!("UDP send failed: {e}"))?;

        let mut tx_guard = self.tx_count.lock().unwrap_or_else(|e| e.into_inner());
        *tx_guard += 1;

        // Update interface stats
        let mut iface_guard = self.interfaces.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(iface) = iface_guard
            .iter_mut()
            .find(|i| i.kind == ReticulumInterfaceKind::UdpUnicast)
        {
            iface.tx_packets += 1;
        }

        Ok(())
    }

    /// Broadcasts a Data packet to all known peers, returning the number sent to.
    pub fn broadcast_data(&self, payload: Vec<u8>) -> usize {
        if payload.len() > 262144 {
            return 0;
        }
        let known = self.known_peers();
        if known.is_empty() {
            return 0;
        }
        let pkt = ReticulumPacket::new(
            ReticulumAddress::from_bytes([0xffu8; 16]),
            ReticulumPacketType::Data,
            payload,
        );
        for peer in &known {
            let _ = self.send_packet(*peer, &pkt);
        }
        known.len()
    }

    /// Drains and returns all queued inbound Data payloads.
    pub fn drain_delivered(&self) -> Vec<Vec<u8>> {
        let mut guard = self.delivered.lock().unwrap_or_else(|e| e.into_inner());
        guard.drain(..).collect()
    }

    /// Returns a sorted copy of the known peer addresses.
    pub fn known_peers(&self) -> Vec<SocketAddr> {
        let mut peers: Vec<SocketAddr> = self
            .peers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .copied()
            .collect();
        peers.sort();
        peers
    }

    /// Number of known peer addresses.
    pub fn peers_count(&self) -> usize {
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Sends a unicast Data packet to a specific peer.
    pub fn send_data_to(&self, peer: SocketAddr, payload: Vec<u8>) -> Result<(), String> {
        if payload.len() > 262144 {
            return Err("Data payload exceeds 256 KiB cap".to_string());
        }
        let pkt = ReticulumPacket::new(self.destination, ReticulumPacketType::Data, payload);
        self.send_packet(peer, &pkt)
    }

    pub fn get_status(&self) -> ReticulumNodeStatus {
        let running = *self.running.lock().unwrap_or_else(|e| e.into_inner());
        let rx_packets = *self.rx_count.lock().unwrap_or_else(|e| e.into_inner());
        let tx_packets = *self.tx_count.lock().unwrap_or_else(|e| e.into_inner());
        let path_guard = self.path_table.lock().unwrap_or_else(|e| e.into_inner());
        let active_routes = path_guard.len();
        let interfaces = self
            .interfaces
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        ReticulumNodeStatus {
            running,
            destination_hash: self.destination.to_hex(),
            active_routes,
            rx_packets,
            tx_packets,
            interfaces,
        }
    }

    /// Broadcasts an ANNOUNCE packet for identity discovery across Reticulum mesh paths.
    pub fn create_announce(&self, aspect: Option<&str>) -> ReticulumPacket {
        let mut payload = Vec::new();
        payload.extend_from_slice(self.destination.0.as_ref());
        if let Some(asp) = aspect {
            payload.extend_from_slice(asp.as_bytes());
        }
        let mut tx_guard = self.tx_count.lock().unwrap_or_else(|e| e.into_inner());
        *tx_guard += 1;

        ReticulumPacket::new(self.destination, ReticulumPacketType::Announce, payload)
    }

    /// Processes an inbound Reticulum packet, updating mesh routing tables when appropriate.
    pub fn process_packet(&self, pkt: &ReticulumPacket) -> Option<ReticulumPacket> {
        let mut rx_guard = self.rx_count.lock().unwrap_or_else(|e| e.into_inner());
        *rx_guard += 1;

        let now_secs = now_secs() as u64;

        if pkt.packet_type == ReticulumPacketType::Announce {
            let mut path_guard = self.path_table.lock().unwrap_or_else(|e| e.into_inner());
            path_guard.update_route(pkt.destination, pkt.destination, pkt.hops, now_secs);
        }

        if pkt.destination != self.destination {
            // Forward packet if hops remain
            pkt.increment_hops()
        } else {
            None
        }
    }

    /// Stops the Reticulum node and cleans up transport resources.
    pub fn stop(&mut self) {
        // Scope the guard: the transport thread re-checks `running` on every
        // iteration, so holding the lock across `join()` below deadlocks.
        {
            let mut run_guard = self.running.lock().unwrap_or_else(|e| e.into_inner());
            *run_guard = false;
        }

        if let Some(handle) = self.transport_thread.take() {
            let _ = handle.join();
        }

        if let Some(mut auto) = self.auto_interface.take() {
            auto.stop();
        }

        if let Some(mut tcp) = self.tcp_server.take() {
            tcp.stop();
        }

        self.udp_socket = None;
    }
}

impl Drop for ReticulumNode {
    fn drop(&mut self) {
        self.stop();
    }
}

static NODES: OnceLock<Mutex<HashMap<String, Arc<Mutex<ReticulumNode>>>>> = OnceLock::new();

/// Returns a shared node for the given pubkey, creating it on first use.
pub fn node_for(pubkey: &str) -> Result<Arc<Mutex<ReticulumNode>>, String> {
    let map = NODES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(node) = guard.get(pubkey) {
        return Ok(node.clone());
    }
    let node = Arc::new(Mutex::new(ReticulumNode::new(pubkey)));
    guard.insert(pubkey.to_string(), node.clone());
    Ok(node)
}

/// Clears the node registry and terminates all background transport threads and interfaces.
pub fn reset_nodes() {
    let map = NODES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    for node in guard.values() {
        if let Ok(mut n) = node.lock() {
            n.stop();
        }
    }
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reticulum::slip_encode;

    /// Serializes tests that touch the process-global NODES registry.
    static TRANSPORT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_reticulum_node_announce_and_process() {
        let node = ReticulumNode::new("test_pubkey_1");
        let status = node.get_status();
        assert!(status.running);
        assert_eq!(status.rx_packets, 0);

        let pkt = node.create_announce(Some("feed"));
        assert_eq!(pkt.packet_type, ReticulumPacketType::Announce);

        let node2 = ReticulumNode::new("test_pubkey_2");
        let forwarded = node2.process_packet(&pkt);
        assert!(forwarded.is_some());
        let status2 = node2.get_status();
        assert_eq!(status2.rx_packets, 1);
        assert_eq!(status2.active_routes, 1);
    }

    #[test]
    fn test_node_for_creates_and_reuses() {
        let _g = TRANSPORT_TEST_LOCK.lock().unwrap();
        reset_nodes();
        let a1 = node_for("pk_a").unwrap();
        let a2 = node_for("pk_a").unwrap();
        let b = node_for("pk_b").unwrap();
        assert!(Arc::ptr_eq(&a1, &a2));
        assert!(!Arc::ptr_eq(&a1, &b));
        reset_nodes();
    }

    #[test]
    fn test_reset_nodes_drops_registry() {
        let _g = TRANSPORT_TEST_LOCK.lock().unwrap();
        reset_nodes();
        let before = node_for("pk_x").unwrap();
        reset_nodes();
        let after = node_for("pk_x").unwrap();
        assert!(!Arc::ptr_eq(&before, &after));
        reset_nodes();
    }

    #[test]
    fn test_stop_fresh_node() {
        let mut node = ReticulumNode::new("test_pubkey_stop");
        node.stop();
        assert!(!*node.running.lock().unwrap_or_else(|e| e.into_inner()));
    }

    #[test]
    fn test_start_tcp_server_disabled() {
        let mut node = ReticulumNode::new("test_pubkey_tcp");
        let config = TcpInterfaceConfig {
            enabled: false,
            ..Default::default()
        };
        node.start_tcp_server(config).unwrap();
        assert_eq!(node.interfaces.lock().unwrap().len(), 2);
        node.stop();
    }

    #[test]
    fn test_start_auto_interface_disabled() {
        let mut node = ReticulumNode::new("test_pubkey_auto");
        let config = AutoInterfaceConfig {
            enabled: false,
            ..Default::default()
        };
        node.start_auto_interface(config).unwrap();
        assert_eq!(node.interfaces.lock().unwrap().len(), 2);
        node.stop();
    }

    #[test]
    fn test_start_udp_transport_receives_announce() {
        let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let mut node = ReticulumNode::new("test_pubkey_udp");
        node.start_udp_transport(&format!("127.0.0.1:{port}"))
            .unwrap();
        assert_eq!(node.interfaces.lock().unwrap().len(), 1);

        let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let announce = node.create_announce(Some("udp-test"));
        sender
            .send_to(
                &slip_encode(&announce.to_bytes()),
                format!("127.0.0.1:{port}"),
            )
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let rx = *node.rx_count.lock().unwrap_or_else(|e| e.into_inner());
            if rx >= 1 {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("udp transport never received announce");
            }
            thread::sleep(Duration::from_millis(20));
        }

        node.stop();
        assert!(!*node.running.lock().unwrap_or_else(|e| e.into_inner()));
        assert!(node.udp_socket.is_none());
    }

    #[test]
    fn test_process_packet_non_announce_skips_routing() {
        let node = ReticulumNode::new("test_pubkey_non_announce");
        let other = ReticulumAddress::from_pubkey("other_pubkey");
        let pkt = ReticulumPacket::new(other, ReticulumPacketType::LinkRequest, vec![]);

        let forwarded = node.process_packet(&pkt).unwrap();
        assert_eq!(forwarded.packet_type, ReticulumPacketType::LinkRequest);
        assert_eq!(forwarded.hops, 1);

        // Non-Announce packets must not touch the path table
        assert_eq!(node.path_table.lock().unwrap().len(), 0);
        assert_eq!(*node.rx_count.lock().unwrap_or_else(|e| e.into_inner()), 1);
    }

    #[test]
    fn test_process_packet_self_addressed_returns_none() {
        let node = ReticulumNode::new("test_pubkey_self");
        let pkt = ReticulumPacket::new(node.destination, ReticulumPacketType::Announce, vec![]);

        assert!(node.process_packet(&pkt).is_none());
        assert_eq!(*node.rx_count.lock().unwrap_or_else(|e| e.into_inner()), 1);
    }

    #[test]
    fn test_send_packet_without_transport_errors() {
        let node = ReticulumNode::new("test_pubkey_no_udp");
        let pkt = ReticulumPacket::new(node.destination, ReticulumPacketType::Announce, vec![]);

        let err = node
            .send_packet("127.0.0.1:4242".parse().unwrap(), &pkt)
            .unwrap_err();
        assert_eq!(err, "UDP transport not started");
        assert_eq!(*node.tx_count.lock().unwrap_or_else(|e| e.into_inner()), 0);
    }

    fn poll_until(mut cond: impl FnMut() -> bool, what: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if cond() {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("timeout waiting for {what}");
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn bound_addr(node: &ReticulumNode) -> SocketAddr {
        node.udp_socket.as_ref().unwrap().local_addr().unwrap()
    }

    #[test]
    fn test_data_broadcast_delivery() {
        let mut node_a = ReticulumNode::new("test_pubkey_bcast_a");
        node_a.start_udp_transport("127.0.0.1:0").unwrap();
        let a_addr = bound_addr(&node_a);

        let mut node_b = ReticulumNode::new("test_pubkey_bcast_b");
        node_b.start_udp_transport("127.0.0.1:0").unwrap();
        let _b_addr = bound_addr(&node_b);

        node_b.send_data_to(a_addr, vec![]).unwrap();
        poll_until(|| node_a.peers_count() >= 1, "A to learn B");

        let sent = node_a.broadcast_data(b"hello".to_vec());
        assert_eq!(sent, 1);

        let mut got = Vec::new();
        poll_until(
            || {
                got = node_b.drain_delivered();
                !got.is_empty()
            },
            "B to deliver broadcast",
        );
        assert_eq!(got, vec![b"hello".to_vec()]);

        node_a.stop();
        node_b.stop();
    }

    #[test]
    fn test_data_forwarding() {
        let mut node_a = ReticulumNode::new("test_pubkey_fwd_a");
        node_a.start_udp_transport("127.0.0.1:0").unwrap();
        let a_addr = bound_addr(&node_a);

        let mut node_b = ReticulumNode::new("test_pubkey_fwd_b");
        node_b.start_udp_transport("127.0.0.1:0").unwrap();
        let b_addr = bound_addr(&node_b);

        let mut node_c = ReticulumNode::new("test_pubkey_fwd_c");
        node_c.start_udp_transport("127.0.0.1:0").unwrap();

        node_c.send_data_to(b_addr, b"seed".to_vec()).unwrap();
        node_b.send_data_to(a_addr, b"peer".to_vec()).unwrap();
        poll_until(
            || node_a.peers_count() >= 1 && node_b.peers_count() >= 1,
            "A and B to learn peers",
        );

        let sent = node_a.broadcast_data(b"msg".to_vec());
        assert_eq!(sent, 1);

        let mut got_b = Vec::new();
        poll_until(
            || {
                got_b = node_b.drain_delivered();
                !got_b.is_empty()
            },
            "B to deliver msg",
        );
        assert_eq!(got_b, vec![b"msg".to_vec()]);

        let mut got_c = Vec::new();
        poll_until(
            || {
                got_c = node_c.drain_delivered();
                !got_c.is_empty()
            },
            "C to receive forwarded msg",
        );
        assert_eq!(got_c, vec![b"msg".to_vec()]);

        node_a.stop();
        node_b.stop();
        node_c.stop();
    }
}
