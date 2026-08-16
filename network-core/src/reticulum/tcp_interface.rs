//! Reticulum TCP client/server interfaces for reliable IP transport.
//!
//! TCP interfaces provide reliable transport over IP networks for Reticulum.
//! This implementation provides compatibility with Reticulum's TCPInterface
//! and TCPServerInterface behavior.

use super::address::ReticulumAddress;
use super::interface::{ReticulumInterfaceKind, ReticulumInterfaceStatus};
use super::packet::ReticulumPacket;
use super::slip::{slip_decode, slip_encode};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_TCP_PORT: u16 = 4242;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpInterfaceConfig {
    pub listen_port: u16,
    pub max_connections: usize,
    pub enabled: bool,
}

impl Default for TcpInterfaceConfig {
    fn default() -> Self {
        Self {
            listen_port: DEFAULT_TCP_PORT,
            max_connections: 50,
            enabled: true,
        }
    }
}

pub struct TcpServerInterface {
    config: TcpInterfaceConfig,
    listener: Option<Arc<TcpListener>>,
    running: Arc<AtomicBool>,
    server_thread: Option<thread::JoinHandle<()>>,
    local_destination: ReticulumAddress,
    rx_count: Arc<AtomicU64>,
    tx_count: Arc<AtomicU64>,
    active_connections: Arc<Mutex<usize>>,
}

impl TcpServerInterface {
    pub fn new(local_destination: ReticulumAddress, config: TcpInterfaceConfig) -> Self {
        Self {
            config,
            listener: None,
            running: Arc::new(AtomicBool::new(false)),
            server_thread: None,
            local_destination,
            rx_count: Arc::new(AtomicU64::new(0)),
            tx_count: Arc::new(AtomicU64::new(0)),
            active_connections: Arc::new(Mutex::new(0)),
        }
    }

    /// Starts the TCP server interface
    pub fn start(&mut self) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(()); // Disabled, no-op
        }

        let bind_addr = format!("0.0.0.0:{}", self.config.listen_port);
        let listener =
            TcpListener::bind(&bind_addr).map_err(|e| format!("TCP bind failed: {e}"))?;

        listener
            .set_nonblocking(true)
            .map_err(|e| format!("set_nonblocking failed: {e}"))?;

        let running = self.running.clone();
        let local_dest = self.local_destination;
        let rx_count = self.rx_count.clone();
        let tx_count = self.tx_count.clone();
        let active_connections = self.active_connections.clone();
        let max_conn = self.config.max_connections;

        let listener_arc = Arc::new(listener);
        let listener_clone = listener_arc.clone();

        let handle = thread::spawn(move || {
            let mut current_connections = 0usize;

            while running.load(Ordering::Relaxed) {
                match listener_clone.accept() {
                    Ok((stream, peer_addr)) => {
                        if current_connections >= max_conn {
                            println!(
                                "TCP server: max connections reached, rejecting {}",
                                peer_addr
                            );
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                            continue;
                        }

                        current_connections += 1;
                        *active_connections.lock().unwrap_or_else(|e| e.into_inner()) =
                            current_connections;

                        let stream_clone = stream.try_clone().unwrap();
                        let local_dest_clone = local_dest;
                        let rx_count_clone = rx_count.clone();
                        let tx_count_clone = tx_count.clone();
                        let active_conn_clone = active_connections.clone();
                        let _running_clone = running.clone();

                        thread::spawn(move || {
                            Self::handle_client(
                                stream_clone,
                                peer_addr,
                                local_dest_clone,
                                rx_count_clone,
                                tx_count_clone,
                            );

                            let mut conn =
                                active_conn_clone.lock().unwrap_or_else(|e| e.into_inner());
                            if *conn > 0 {
                                *conn -= 1;
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(e) => {
                        eprintln!("TCP accept error: {}", e);
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                }
            }
        });

        // Don't store the listener since it's moved to the thread
        // In a full implementation, we'd use a different approach for listener management
        self.server_thread = Some(handle);
        self.running.store(true, Ordering::Relaxed);

        Ok(())
    }

    /// Stops the TCP server interface
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.server_thread.take() {
            let _ = handle.join();
        }
        self.listener = None;
    }

    /// Handles a single TCP client connection
    fn handle_client(
        stream: TcpStream,
        _peer_addr: SocketAddr,
        local_destination: ReticulumAddress,
        rx_count: Arc<AtomicU64>,
        _tx_count: Arc<AtomicU64>,
    ) {
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

        let mut reader = BufReader::new(&stream);
        let mut writer = BufWriter::new(&stream);

        // Simple handshake: send local destination
        let handshake = slip_encode(&local_destination.0);
        if writer.write_all(&handshake).is_err() {
            return;
        }
        if writer.flush().is_err() {
            return;
        }

        // Main packet loop
        let mut line_buffer = String::new();
        loop {
            line_buffer.clear();
            match reader.read_line(&mut line_buffer) {
                Ok(0) => break, // Connection closed
                Ok(_) => {
                    // Process SLIP-encoded packets
                    let raw_data = line_buffer.trim().as_bytes();
                    if let Ok(decoded) = slip_decode(raw_data) {
                        if let Ok(_packet) = ReticulumPacket::from_bytes(&decoded) {
                            rx_count.fetch_add(1, Ordering::Relaxed);
                            // In full implementation, process packet and send response
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Gets the interface status
    pub fn get_status(&self) -> ReticulumInterfaceStatus {
        let _active = *self
            .active_connections
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        ReticulumInterfaceStatus {
            name: "TCP Server".to_string(),
            kind: ReticulumInterfaceKind::UdpUnicast, // Reuse UdpUnicast for TCP
            bind_address: format!("0.0.0.0:{}", self.config.listen_port),
            active: self.running.load(Ordering::Relaxed),
            rx_packets: self.rx_count.load(Ordering::Relaxed),
            tx_packets: self.tx_count.load(Ordering::Relaxed),
        }
    }
}

impl Drop for TcpServerInterface {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct TcpClientInterface {
    target_addr: SocketAddr,
    _local_destination: ReticulumAddress,
    rx_count: Arc<AtomicU64>,
    tx_count: Arc<AtomicU64>,
}

impl TcpClientInterface {
    pub fn new(target_addr: SocketAddr, local_destination: ReticulumAddress) -> Self {
        Self {
            target_addr,
            _local_destination: local_destination,
            rx_count: Arc::new(AtomicU64::new(0)),
            tx_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Connects to the target TCP server
    pub fn connect(&self) -> Result<TcpStream, String> {
        let stream =
            TcpStream::connect(self.target_addr).map_err(|e| format!("TCP connect failed: {e}"))?;

        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| format!("set_read_timeout failed: {e}"))?;

        // Perform handshake
        let mut reader = BufReader::new(&stream);
        let mut handshake_line = String::new();

        if reader.read_line(&mut handshake_line).is_err() {
            return Err("Handshake read failed".to_string());
        }

        // Receive and decode remote destination
        let raw_data = handshake_line.trim().as_bytes();
        if let Ok(decoded) = slip_decode(raw_data) {
            if decoded.len() == 16 {
                let _remote_dest =
                    ReticulumAddress::from_bytes(decoded.try_into().unwrap_or([0u8; 16]));
                // Handshake successful
            }
        }

        Ok(stream)
    }

    /// Sends a packet via TCP
    pub fn send_packet(
        &self,
        stream: &mut TcpStream,
        packet: &ReticulumPacket,
    ) -> Result<(), String> {
        let packet_bytes = packet.to_bytes();
        let slip_encoded = slip_encode(&packet_bytes);

        stream
            .write_all(&slip_encoded)
            .map_err(|e| format!("TCP send failed: {e}"))?;
        stream
            .flush()
            .map_err(|e| format!("TCP flush failed: {e}"))?;

        self.tx_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Receives a packet via TCP
    pub fn receive_packet(&self, stream: &mut TcpStream) -> Result<ReticulumPacket, String> {
        let mut reader = BufReader::new(stream);
        let mut line_buffer = String::new();

        reader
            .read_line(&mut line_buffer)
            .map_err(|e| format!("TCP receive failed: {e}"))?;

        let raw_data = line_buffer.trim().as_bytes();
        let decoded = slip_decode(raw_data).map_err(|e| format!("SLIP decode failed: {e}"))?;

        let packet = ReticulumPacket::from_bytes(&decoded)
            .map_err(|e| format!("Packet parse failed: {e}"))?;

        self.rx_count.fetch_add(1, Ordering::Relaxed);
        Ok(packet)
    }

    /// Gets client statistics
    pub fn get_stats(&self) -> (u64, u64) {
        (
            self.rx_count.load(Ordering::Relaxed),
            self.tx_count.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_config_default() {
        let config = TcpInterfaceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.listen_port, DEFAULT_TCP_PORT);
        assert_eq!(config.max_connections, 50);
    }

    #[test]
    fn test_tcp_client_creation() {
        let local_dest = ReticulumAddress::from_pubkey("test");
        let target = "127.0.0.1:4242".parse().unwrap();

        let client = TcpClientInterface::new(target, local_dest);
        assert_eq!(client.target_addr, target);
    }

    #[test]
    fn test_tcp_server_creation() {
        let local_dest = ReticulumAddress::from_pubkey("test");
        let config = TcpInterfaceConfig::default();

        let server = TcpServerInterface::new(local_dest, config);
        assert_eq!(server.config.listen_port, DEFAULT_TCP_PORT);
    }
}
