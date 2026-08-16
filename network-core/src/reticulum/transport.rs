//! Reticulum mesh network node coordinator and social transport adapter.

use super::address::ReticulumAddress;
use super::auto_interface::{AutoInterface, AutoInterfaceConfig};
use super::interface::{ReticulumInterfaceKind, ReticulumInterfaceStatus};
use super::link::LinkManager;
use super::packet::{ReticulumPacket, ReticulumPacketType};
use super::routing::PathTable;
use super::tcp_interface::{TcpInterfaceConfig, TcpServerInterface};
use serde::{Deserialize, Serialize};
use soshal_common_core::format::now_secs;
use std::collections::HashMap;
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
        let _destination = self.destination;
        let link_manager = self.link_manager.clone();

        let udp_clone = udp_socket.clone();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 2048];

            while *running.lock().unwrap_or_else(|e| e.into_inner()) {
                match udp_clone.recv_from(&mut buf) {
                    Ok((len, _src_addr)) => {
                        if let Ok(packet_data) = super::slip::slip_decode(&buf[..len]) {
                            if let Ok(pkt) = ReticulumPacket::from_bytes(&packet_data) {
                                let mut rx_guard =
                                    rx_count.lock().unwrap_or_else(|e| e.into_inner());
                                *rx_guard += 1;

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
                                    let _ = link_manager.handle_link_request(pkt.destination);
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
        let mut run_guard = self.running.lock().unwrap_or_else(|e| e.into_inner());
        *run_guard = false;

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

/// Clears the node registry; running transport threads persist until process exit.
pub fn reset_nodes() {
    let map = NODES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
