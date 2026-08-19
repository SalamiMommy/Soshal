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
use std::io::{BufWriter, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_TCP_PORT: u16 = 4242;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Matches `lan_transport::MAX_FRAME_BYTES`; caps hostile length-prefix
/// headers (u32 BE) so a 0xFFFFFFFF header cannot trigger a 4 GiB alloc.
const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Frame a payload with a u32 BE length prefix. SLIP frames are binary
/// (0xC0/0xDB are invalid UTF-8), so `read_line`-based framing can never
/// work — every read site uses this length prefix instead.
fn frame_payload(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Read one length-prefixed frame from a reader. Uses `read_exact` on the
/// raw stream (never a `BufReader`): buffered readers over-read past the
/// frame boundary and later reads on a fresh reader lose the buffered bytes.
fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut len_bytes = [0u8; 4];
    reader
        .read_exact(&mut len_bytes)
        .map_err(|e| format!("frame header read failed: {e}"))?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(format!(
            "frame too large: {len} bytes (max {MAX_FRAME_BYTES})"
        ));
    }
    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .map_err(|e| format!("frame body read failed: {e}"))?;
    Ok(payload)
}

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
        let listener_clone = listener_arc;

        // Set running BEFORE spawning: the accept loop exits immediately
        // when the flag is false, so spawning first races the store below
        // and can kill the server thread before any connection is accepted.
        self.running.store(true, Ordering::Relaxed);

        let handle = thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                match listener_clone.accept() {
                    Ok((stream, peer_addr)) => {
                        // Gate + increment on the SHARED counter: the client
                        // thread decrements it when the connection ends, so
                        // max_connections frees up after clients disconnect.
                        let mut current =
                            active_connections.lock().unwrap_or_else(|e| e.into_inner());
                        if *current >= max_conn {
                            drop(current);
                            println!(
                                "TCP server: max connections reached, rejecting {}",
                                peer_addr
                            );
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                            continue;
                        }
                        *current += 1;
                        drop(current);

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

        let mut writer = BufWriter::new(&stream);

        // Simple handshake: send local destination
        let handshake = frame_payload(&slip_encode(&local_destination.0));
        if writer.write_all(&handshake).is_err() {
            return;
        }
        if writer.flush().is_err() {
            return;
        }

        // Main packet loop
        while let Ok(raw_data) = read_frame(&mut &stream) {
            if let Ok(decoded) = slip_decode(&raw_data) {
                if let Ok(_packet) = ReticulumPacket::from_bytes(&decoded) {
                    rx_count.fetch_add(1, Ordering::Relaxed);
                    // In full implementation, process packet and send response
                }
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
        let raw_data = match read_frame(&mut &stream) {
            Ok(d) => d,
            Err(e) => return Err(format!("Handshake read failed: {e}")),
        };

        // Receive and decode remote destination
        if let Ok(decoded) = slip_decode(&raw_data) {
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
        let framed = frame_payload(&slip_encode(&packet_bytes));

        stream
            .write_all(&framed)
            .map_err(|e| format!("TCP send failed: {e}"))?;
        stream
            .flush()
            .map_err(|e| format!("TCP flush failed: {e}"))?;

        self.tx_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Receives a packet via TCP
    pub fn receive_packet(&self, stream: &mut TcpStream) -> Result<ReticulumPacket, String> {
        let raw_data = read_frame(&mut &*stream).map_err(|e| format!("TCP receive failed: {e}"))?;

        let decoded = slip_decode(&raw_data).map_err(|e| format!("SLIP decode failed: {e}"))?;

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
    use crate::reticulum::packet::ReticulumPacketType;

    #[test]
    fn test_read_frame_rejects_oversized_header() {
        // 0xFFFFFFFF header must error before any allocation
        let mut reader = std::io::Cursor::new(vec![0xFF, 0xFF, 0xFF, 0xFF]);
        let err = read_frame(&mut reader).unwrap_err();
        assert!(err.contains("frame too large"));

        // Boundary: exactly MAX_FRAME_BYTES is accepted (header only, no body)
        let mut reader = std::io::Cursor::new((MAX_FRAME_BYTES as u32).to_be_bytes().to_vec());
        let err = read_frame(&mut reader).unwrap_err();
        assert!(err.contains("frame body read failed"));
    }

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

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn connect_with_retry(client: &TcpClientInterface) -> TcpStream {
        for _ in 0..20 {
            if let Ok(stream) = client.connect() {
                return stream;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("client connect never succeeded");
    }

    #[test]
    fn test_tcp_server_disabled_noop() {
        let local_dest = ReticulumAddress::from_pubkey("test");
        let config = TcpInterfaceConfig {
            enabled: false,
            ..Default::default()
        };
        let mut server = TcpServerInterface::new(local_dest, config);
        assert!(server.start().is_ok());
        assert!(!server.running.load(Ordering::Relaxed));
        assert!(!server.get_status().active);
    }

    #[test]
    fn test_tcp_server_bind_failure() {
        let occupied = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        let config = TcpInterfaceConfig {
            listen_port: port,
            ..Default::default()
        };
        let mut server = TcpServerInterface::new(ReticulumAddress::from_pubkey("test"), config);
        let err = server.start().unwrap_err();
        assert!(err.contains("TCP bind failed"));
    }

    #[test]
    fn test_tcp_client_connect_refused() {
        let port = free_port();
        let client = TcpClientInterface::new(
            format!("127.0.0.1:{port}").parse().unwrap(),
            ReticulumAddress::from_pubkey("test"),
        );
        let err = client.connect().unwrap_err();
        assert!(err.contains("TCP connect failed"));
    }

    #[test]
    fn test_tcp_client_handshake_and_receive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let remote_dest = ReticulumAddress::from_pubkey("mock_server");

        let handle = thread::spawn(move || {
            let (stream, _peer) = listener.accept().expect("mock accept failed");
            let mut writer = BufWriter::new(&stream);
            let handshake = frame_payload(&slip_encode(&remote_dest.0));
            writer.write_all(&handshake).unwrap();
            writer.flush().unwrap();
            let packet =
                ReticulumPacket::new(remote_dest, ReticulumPacketType::Data, vec![1, 2, 3, 4]);
            let framed = frame_payload(&slip_encode(&packet.to_bytes()));
            writer.write_all(&framed).unwrap();
            writer.flush().unwrap();
            thread::sleep(Duration::from_secs(30));
        });

        let client = TcpClientInterface::new(
            format!("127.0.0.1:{port}").parse().unwrap(),
            ReticulumAddress::from_pubkey("test"),
        );
        let mut stream = connect_with_retry(&client);

        let received = client.receive_packet(&mut stream).unwrap();
        assert_eq!(received.packet_type, ReticulumPacketType::Data);
        assert_eq!(received.payload, vec![1, 2, 3, 4]);
        assert_eq!(client.get_stats(), (1, 0));

        drop(handle);
    }

    #[test]
    fn test_tcp_server_client_roundtrip() {
        let port = free_port();
        let local_dest = ReticulumAddress::from_pubkey("test");
        let config = TcpInterfaceConfig {
            listen_port: port,
            ..Default::default()
        };
        let mut server = TcpServerInterface::new(local_dest, config);
        server.start().unwrap();
        assert!(server.get_status().active);

        let client =
            TcpClientInterface::new(format!("127.0.0.1:{port}").parse().unwrap(), local_dest);
        let mut stream = connect_with_retry(&client);

        let packet = ReticulumPacket::new(local_dest, ReticulumPacketType::Data, vec![9, 8, 7]);
        client.send_packet(&mut stream, &packet).unwrap();
        assert_eq!(client.get_stats(), (0, 1));

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if server.get_status().rx_packets >= 1 {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("server never received packet");
            }
            thread::sleep(Duration::from_millis(20));
        }

        drop(stream);
        server.stop();
        assert!(!server.get_status().active);
    }

    #[test]
    fn test_tcp_server_max_connections_reject() {
        let port = free_port();
        let config = TcpInterfaceConfig {
            listen_port: port,
            max_connections: 1,
            ..Default::default()
        };
        let mut server = TcpServerInterface::new(ReticulumAddress::from_pubkey("test"), config);
        server.start().unwrap();

        let client = TcpClientInterface::new(
            format!("127.0.0.1:{port}").parse().unwrap(),
            ReticulumAddress::from_pubkey("test"),
        );
        let _first = connect_with_retry(&client);
        thread::sleep(Duration::from_millis(200));

        let second = client.connect();
        match second {
            Err(e) => assert!(e.contains("Handshake read failed")),
            Ok(s) => {
                assert!(read_frame(&mut &s).is_err());
            }
        }

        server.stop();
    }
}
