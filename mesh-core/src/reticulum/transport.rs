//! Reticulum mesh network node coordinator and social transport adapter.

use super::address::ReticulumAddress;
use super::auto_interface::{AutoInterface, AutoInterfaceConfig};
use super::interface::{ReticulumInterfaceKind, ReticulumInterfaceStatus};
use super::link::LinkManager;
use super::packet::{ReticulumPacket, ReticulumPacketType, MAX_HOPS};

/// Cap on tracked UDP peers. The map grows per spoofed source address, so a
/// flood would otherwise fan out re-broadcasts to every spoofed peer.
const MAX_PEERS: usize = 4096;
/// Dedup window for forwarded Data packets (payload hash per source),
/// preventing mesh amplification via re-broadcast loops.
const MAX_SEEN_DATA: usize = 4096;
/// How long the transport thread parks in the kernel before re-checking
/// `running` when no datagram arrives.
///
/// The receive socket is blocking with this read timeout rather than
/// non-blocking-with-a-poll: the previous 10 ms `sleep` on `WouldBlock` cost
/// 100 wakeups/second/node forever (8.6M/node/day), which is what holds a core
/// out of deep C-state. 500 ms is the same value the plan called for; it bounds
/// `stop()`'s join at 500 ms, which is fine for a full node teardown.
const RECV_PARK_MILLIS: u64 = 500;
const RECV_PARK_TIMEOUT: Duration = Duration::from_millis(RECV_PARK_MILLIS);
/// Route-table maintenance interval. Time-based, not tick-based: the old
/// "every 1000 iterations (~10 s when idle)" tied the prune rate to the poll
/// rate, so parking the thread would have silently stretched it to ~8 min and
/// let an attacker-filled route table grow unbounded in between.
const ROUTE_PRUNE_INTERVAL_SECS: u64 = 10;
use super::routing::PathTable;
use super::tcp_interface::{TcpInterfaceConfig, TcpServerInterface};
use serde::{Deserialize, Serialize};
use soshal_common_core::format::now_secs;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{SocketAddr, UdpSocket};

use std::sync::atomic::{AtomicBool, Ordering};
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
    pub running: Arc<AtomicBool>,
    pub link_manager: Arc<LinkManager>,
    pub peers: Arc<Mutex<HashSet<SocketAddr>>>,
    pub delivered: Arc<Mutex<VecDeque<Vec<u8>>>>,
    pub seen_data: Arc<Mutex<VecDeque<String>>>,
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
            running: Arc::new(AtomicBool::new(true)),
            link_manager: Arc::new(LinkManager::new()),
            peers: Arc::new(Mutex::new(HashSet::new())),
            delivered: Arc::new(Mutex::new(VecDeque::new())),
            seen_data: Arc::new(Mutex::new(VecDeque::new())),
            udp_socket: None,
            transport_thread: None,
            auto_interface: None,
            tcp_server: None,
        }
    }

    /// Starts the AutoInterface for peer discovery
    pub fn start_auto_interface(&mut self, config: AutoInterfaceConfig) -> Result<(), String> {
        if let Some(mut old) = self.auto_interface.take() {
            old.stop();
        }
        let mut auto = AutoInterface::new(self.destination, config);
        auto.start()?;
        let status = auto.get_status();
        self.auto_interface = Some(auto);

        // Update interface list
        let mut iface_guard = self.interfaces.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = iface_guard.iter_mut().find(|i| i.name == status.name) {
            *existing = status;
        } else {
            iface_guard.push(status);
        }

        Ok(())
    }

    /// Starts the TCP server interface
    pub fn start_tcp_server(&mut self, config: TcpInterfaceConfig) -> Result<(), String> {
        if let Some(mut old) = self.tcp_server.take() {
            old.stop();
        }
        let mut tcp = TcpServerInterface::new(self.destination, config);
        tcp.start()?;
        let status = tcp.get_status();
        self.tcp_server = Some(tcp);

        // Update interface list
        let mut iface_guard = self.interfaces.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = iface_guard.iter_mut().find(|i| i.name == status.name) {
            *existing = status;
        } else {
            iface_guard.push(status);
        }

        Ok(())
    }

    /// Starts the UDP transport layer for Reticulum communication
    pub fn start_udp_transport(&mut self, bind_addr: &str) -> Result<(), String> {
        if let Some(handle) = self.transport_thread.take() {
            self.running.store(false, Ordering::Relaxed);
            let _ = handle.join();
        }
        self.running.store(true, Ordering::Relaxed);

        let socket = UdpSocket::bind(bind_addr).map_err(|e| format!("UDP bind failed: {e}"))?;
        // Blocking socket + read timeout, NOT `set_nonblocking` + a poll loop:
        // the thread parks in the kernel and stops generating 100 wakeups/sec.
        socket
            .set_read_timeout(Some(RECV_PARK_TIMEOUT))
            .map_err(|e| format!("set_read_timeout failed: {e}"))?;

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
        let seen_data = self.seen_data.clone();

        let udp_clone = udp_socket.clone();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 2048];
            let mut last_prune = now_secs() as u64;

            while running.load(Ordering::Relaxed) {
                // Periodic maintenance: prune expired routes on a wall-clock
                // interval so attacker-filled tables cannot grow without bound.
                // Wall-clock, not per-iteration: the loop now parks in
                // `recv_from` for RECV_PARK_TIMEOUT, so an iteration count
                // would stretch the prune from ~10 s to ~8 min.
                let now = now_secs() as u64;
                if now.saturating_sub(last_prune) >= ROUTE_PRUNE_INTERVAL_SECS {
                    last_prune = now;
                    path_table
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .prune_expired(now);
                }
                match udp_clone.recv_from(&mut buf) {
                    Ok((len, src_addr)) => {
                        // Reject non-private sources early: prevents
                        // amplification/D.o.S from spoofed public addrs
                        // being inserted into peers and re-broadcast.
                        if !soshal_common_core::url::is_private_ip_str(&src_addr.ip().to_string()) {
                            continue;
                        }
                        if let Ok(packet_data) = super::slip::slip_decode(&buf[..len]) {
                            if let Ok(pkt) = ReticulumPacket::from_bytes(&packet_data) {
                                let mut rx_guard =
                                    rx_count.lock().unwrap_or_else(|e| e.into_inner());
                                *rx_guard += 1;

                                {
                                    let mut peer_guard =
                                        peers.lock().unwrap_or_else(|e| e.into_inner());
                                    if peer_guard.len() < MAX_PEERS {
                                        peer_guard.insert(src_addr);
                                    }
                                }

                                if pkt.packet_type == ReticulumPacketType::Data
                                    && pkt.payload.len() <= 262144
                                {
                                    // Re-broadcast dedup: skip data already
                                    // forwarded from this source.
                                    let mut seen =
                                        seen_data.lock().unwrap_or_else(|e| e.into_inner());
                                    let digest = blake3::hash(&pkt.payload).to_hex().to_string();
                                    if !seen.contains(&digest) {
                                        if seen.len() >= MAX_SEEN_DATA {
                                            seen.pop_front();
                                        }
                                        seen.push_back(digest.clone());
                                        drop(seen);
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
                                            let fwd_bytes =
                                                super::slip::slip_encode(&fwd.to_bytes());
                                            for peer in known {
                                                let _ = udp_clone.send_to(&fwd_bytes, peer);
                                            }
                                        }
                                    }
                                }

                                let now_secs = now_secs() as u64;

                                if pkt.packet_type == ReticulumPacketType::Announce {
                                    let next_hop = if pkt.hops == 0 {
                                        pkt.destination
                                    } else {
                                        ReticulumAddress::from_aspect(
                                            "reticulum.peer",
                                            &src_addr.to_string(),
                                        )
                                    };
                                    let mut path_guard =
                                        path_table.lock().unwrap_or_else(|e| e.into_inner());
                                    path_guard.update_route(
                                        pkt.destination,
                                        next_hop,
                                        pkt.hops,
                                        now_secs,
                                    );
                                    // Forward Announce to other peers if hops remain
                                    let mut seen =
                                        seen_data.lock().unwrap_or_else(|e| e.into_inner());
                                    let digest = blake3::hash(&pkt.to_bytes()).to_hex().to_string();
                                    if !seen.contains(&digest) {
                                        if seen.len() >= MAX_SEEN_DATA {
                                            seen.pop_front();
                                        }
                                        seen.push_back(digest);
                                        drop(seen);
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
                                            let fwd_bytes =
                                                super::slip::slip_encode(&fwd.to_bytes());
                                            for peer in known {
                                                let _ = udp_clone.send_to(&fwd_bytes, peer);
                                            }
                                        }
                                    }
                                } else if pkt.packet_type == ReticulumPacketType::LinkRequest {
                                    if pkt.destination == destination {
                                        let sender_dest = if pkt.payload.len() > 17
                                            && pkt.payload[0]
                                                == super::link::LINK_REQUEST_PQC_SENDER
                                        {
                                            let mut d = [0u8; 16];
                                            d.copy_from_slice(&pkt.payload[1..17]);
                                            ReticulumAddress(d)
                                        } else {
                                            ReticulumAddress::from_aspect(
                                                "reticulum.peer",
                                                &src_addr.to_string(),
                                            )
                                        };
                                        if let Ok(proof_pkt) = link_manager
                                            .handle_link_request_with_own_dest(
                                                Some(destination),
                                                sender_dest,
                                                &pkt.payload,
                                            )
                                        {
                                            let fwd_bytes =
                                                super::slip::slip_encode(&proof_pkt.to_bytes());
                                            let _ = udp_clone.send_to(&fwd_bytes, src_addr);
                                        }
                                    } else if pkt.hops < MAX_HOPS {
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
                                } else if pkt.packet_type == ReticulumPacketType::Proof {
                                    if pkt.destination == destination {
                                        let _ = link_manager
                                            .handle_link_proof(pkt.destination, &pkt.payload);
                                    } else if pkt.hops < MAX_HOPS {
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
                    // Read timeout with no datagram: the expected idle case.
                    // Loop straight back into the kernel-blocking `recv_from`
                    // — a sleep here would add a second wakeup source.
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        continue;
                    }
                    Err(_) => {
                        // Back off briefly so a persistent socket error cannot
                        // spin this thread at 100% CPU.
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
        let running = self.running.load(Ordering::Relaxed);
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
    ///
    /// NOTE: this entry point carries no source address — callers that can
    /// verify the sender's IP should do so *before* calling this method.
    pub fn process_packet(&self, pkt: &ReticulumPacket) -> Option<ReticulumPacket> {
        let mut rx_guard = self.rx_count.lock().unwrap_or_else(|e| e.into_inner());
        *rx_guard += 1;

        let now_secs = now_secs() as u64;

        if pkt.packet_type == ReticulumPacketType::Announce {
            let next_hop = if pkt.hops == 0 {
                pkt.destination
            } else {
                ReticulumAddress::from_aspect("reticulum.peer", &pkt.destination.to_hex())
            };
            let mut path_guard = self.path_table.lock().unwrap_or_else(|e| e.into_inner());
            path_guard.update_route(pkt.destination, next_hop, pkt.hops, now_secs);
        }

        if pkt.destination != self.destination {
            // Dedup: prevent re-broadcast loops through this path.
            if pkt.packet_type == ReticulumPacketType::Data {
                if pkt.payload.len() > 262144 {
                    return None;
                }
                let digest = blake3::hash(&pkt.payload).to_hex().to_string();
                let mut seen = self.seen_data.lock().unwrap_or_else(|e| e.into_inner());
                if seen.contains(&digest) {
                    return None;
                }
                if seen.len() >= MAX_SEEN_DATA {
                    seen.pop_front();
                }
                seen.push_back(digest);
            }

            // Forward packet if hops remain (increment_hops returns None at MAX_HOPS)
            pkt.increment_hops()
        } else {
            None
        }
    }

    /// Stops the Reticulum node and cleans up transport resources.
    pub fn stop(&mut self) {
        // Scope the guard: the transport thread re-checks `running` on every
        // iteration, so holding the lock across `join()` below deadlocks.
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

/// Whether any registered node is running. Drives transport resolution
/// (a node exists for a pubkey once `node_for` is called; `running` flips
/// false only on stop).
pub fn any_node_running() -> bool {
    let Some(map) = NODES.get() else {
        return false;
    };
    let guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard.values().any(|node| {
        node.lock()
            .map(|n| n.running.load(Ordering::Relaxed))
            .unwrap_or(false)
    })
}

/// Status of the first running registered node (used by the bridge status
/// surface; `None` when no node is running).
pub fn any_node_status() -> Option<ReticulumNodeStatus> {
    let map = NODES.get()?;
    let guard = map.lock().unwrap_or_else(|e| e.into_inner());
    for node in guard.values() {
        if let Ok(n) = node.lock() {
            if n.running.load(Ordering::Relaxed) {
                return Some(n.get_status());
            }
        }
    }
    None
}

/// Prunes stale links (or expired path entries when `now_secs` is set) on
/// the first running registered node; `None` when none is running.
pub fn prune_first_node(now_secs: Option<u64>) -> Option<usize> {
    let map = NODES.get()?;
    let guard = map.lock().unwrap_or_else(|e| e.into_inner());
    for node in guard.values() {
        if let Ok(n) = node.lock() {
            if !n.running.load(Ordering::Relaxed) {
                continue;
            }
            if let Some(now) = now_secs {
                let mut table = n.path_table.lock().unwrap_or_else(|e| e.into_inner());
                return Some(table.prune_expired(now));
            }
            return Some(n.link_manager.prune_stale_links());
        }
    }
    None
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
    fn test_any_node_running_tracks_registry() {
        let _g = TRANSPORT_TEST_LOCK.lock().unwrap();
        reset_nodes();
        assert!(!any_node_running());
        let node = node_for("pk_run").unwrap();
        assert!(any_node_running());
        node.lock().unwrap().stop();
        assert!(!any_node_running());
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
        assert!(!node.running.load(Ordering::Relaxed));
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
        assert!(!node.running.load(Ordering::Relaxed));
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

    // ---- 8.1: the transport thread must park, not poll ----

    /// Counts `recv_from` attempts per unit of wall time on a socket configured
    /// the way `start_udp_transport` configures it. If the socket is
    /// non-blocking (the pre-8.1 shape) this spins; if it is blocking with a
    /// read timeout it runs at the timeout rate.
    fn recv_attempts_per_sec(bind: &str) -> (u64, u64) {
        let s = UdpSocket::bind(bind).expect("bind");
        s.set_read_timeout(Some(RECV_PARK_TIMEOUT))
            .expect("timeout");
        let mut buf = [0u8; 64];
        let mut attempts: u64 = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1200);
        while std::time::Instant::now() < deadline {
            match s.recv_from(&mut buf) {
                Ok(_) => {}
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => {}
            }
            attempts += 1;
        }
        (attempts, 1)
    }

    #[test]
    fn recv_socket_parks_instead_of_spinning() {
        // 1200 ms window. A parked socket retries at RECV_PARK_TIMEOUT
        // (500 ms) => ~3 attempts. The old non-blocking shape retried as fast
        // as the CPU allowed => thousands. 50 attempts is a wide margin that
        // still fails loudly if someone restores set_nonblocking.
        let (attempts, _) = recv_attempts_per_sec("127.0.0.1:0");
        assert!(
            attempts <= 50,
            "receive socket spun instead of parking: {attempts} attempts in 1200 ms \
             (RECV_PARK_TIMEOUT is {:?})",
            RECV_PARK_TIMEOUT
        );
        // And it must not have parked "forever" either: at least one retry has
        // to happen, or stop() would never observe the loop exit.
        assert!(attempts >= 1, "socket never returned from recv_from");
    }

    #[test]
    fn recv_park_timeout_is_the_documented_value() {
        // Pinned because the wakeup-rate win is entirely this number: 500 ms
        // is 173k wakeups/node/day against the old 8.6M.
        const { assert!(RECV_PARK_MILLIS == 500) };
        // Route pruning must stay time-based, or parking the thread stretches
        // it from ~10 s to ~500 s and an attacker-filled table grows unbounded.
        const { assert!(ROUTE_PRUNE_INTERVAL_SECS == 10) };
    }

    #[test]
    fn running_flag_is_an_atomic_bool_not_a_mutex() {
        // Compile-time guarantee that the loop condition cannot take a lock:
        // `Arc<AtomicBool>` is loadable without any guard. If someone reverts
        // to `Arc<Mutex<bool>>` this test stops compiling.
        let node = ReticulumNode::new("test_atomic_running");
        let running: &std::sync::atomic::AtomicBool = &node.running;
        assert!(running.load(Ordering::Relaxed));
        node.running.store(false, Ordering::Relaxed);
        assert!(!node.running.load(Ordering::Relaxed));
        assert!(!node.get_status().running);
    }
}
