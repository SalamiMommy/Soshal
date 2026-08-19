//! I2P SAM V3 client implementation for anonymous networking.

use crate::pqc_link::{parse_handshake_frame, PqcLinkCrypto, PQ_LINK_CRYPTO};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const SAM_DEFAULT_HOST: &str = "127.0.0.1";
const SAM_DEFAULT_PORT: u16 = 7656;
const SAM_VERSION: &str = "3.1";
const SAM_SIGNATURE_TYPE: &str = "7"; // Ed25519
const SAM_ENCRYPTION_TYPE: &str = "4"; // ECIES-X25519

/// I2P SAM V3 client for anonymous networking
pub struct I2PSamClient {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    session_id: Option<String>,
    destination: Option<String>,
}

impl I2PSamClient {
    /// Creates a new I2P SAM client
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            stream: None,
            session_id: None,
            destination: None,
        }
    }

    /// Creates a client with default SAM settings
    pub fn default_client() -> Self {
        Self::new(SAM_DEFAULT_HOST.to_string(), SAM_DEFAULT_PORT)
    }

    /// Connects to the SAM bridge
    pub fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream =
            TcpStream::connect(&addr).map_err(|e| format!("SAM connection failed: {e}"))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set read timeout failed: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set write timeout failed: {e}"))?;

        self.stream = Some(stream);
        Ok(())
    }

    /// Disconnects from the SAM bridge
    pub fn disconnect(&mut self) -> Result<(), String> {
        if let Some(session_id) = &self.session_id {
            self.send_command(&format!("SESSION CLOSE STYLE=STREAM ID={}", session_id))?;
        }

        if let Some(stream) = self.stream.take() {
            stream
                .shutdown(std::net::Shutdown::Both)
                .map_err(|e| format!("Stream shutdown failed: {e}"))?;
        }

        self.session_id = None;
        self.destination = None;
        Ok(())
    }

    /// Sends a SAM command and reads the response
    fn send_command(&mut self, command: &str) -> Result<String, String> {
        let stream = self.stream.as_mut().ok_or("Not connected to SAM bridge")?;

        stream
            .write_all(command.as_bytes())
            .map_err(|e| format!("Command send failed: {e}"))?;
        stream
            .write_all(b"\n")
            .map_err(|e| format!("Newline send failed: {e}"))?;
        stream.flush().map_err(|e| format!("Flush failed: {e}"))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| format!("Response read failed: {e}"))?;

        Ok(response.trim().to_string())
    }

    /// Performs SAM handshake
    pub fn handshake(&mut self) -> Result<String, String> {
        let command = format!("HELLO VERSION={} MIN=3.0 MAX=3.3", SAM_VERSION);
        let response = self.send_command(&command)?;

        if response.starts_with("HELLO REPLY") {
            Ok(response)
        } else {
            Err(format!("Handshake failed: {}", response))
        }
    }

    /// Generates a new destination
    pub fn generate_destination(&mut self) -> Result<String, String> {
        let command = format!("DEST GENERATE SIGNATURE_TYPE={}", SAM_SIGNATURE_TYPE);
        let response = self.send_command(&command)?;

        if response.starts_with("DEST REPLY") {
            // Parse destination from response
            if let Some(dest) = response.split("DEST=").nth(1) {
                Ok(dest.to_string())
            } else {
                Err("Failed to parse destination".to_string())
            }
        } else {
            Err(format!("Destination generation failed: {}", response))
        }
    }

    /// Creates a new session
    pub fn create_session(
        &mut self,
        session_id: &str,
        destination: Option<&str>,
    ) -> Result<String, String> {
        let dest_str = destination.unwrap_or("TRANSIENT");
        let command = format!(
            "SESSION CREATE STYLE=STREAM ID={} DESTINATION={} SIGNATURE_TYPE={} i2cp.leaseSetEncType={}",
            session_id, dest_str, SAM_SIGNATURE_TYPE, SAM_ENCRYPTION_TYPE
        );

        let response = self.send_command(&command)?;

        if response.starts_with("SESSION STATUS RESULT=OK") {
            // Extract destination from response if transient
            if destination.is_none() {
                match response.split("DESTINATION=").nth(1) {
                    Some(dest) if !dest.is_empty() => {
                        self.session_id = Some(session_id.to_string());
                        self.destination = Some(dest.to_string());
                    }
                    _ => {
                        return Err("Session created without DESTINATION= in response".to_string());
                    }
                }
            } else {
                self.session_id = Some(session_id.to_string());
                self.destination = destination.map(|d| d.to_string());
            }

            Ok(response)
        } else {
            Err(format!("Session creation failed: {}", response))
        }
    }

    /// Connects to a remote I2P destination
    pub fn connect_to_destination(&mut self, destination: &str) -> Result<TcpStream, String> {
        if let Some(session_id) = &self.session_id {
            let command = format!(
                "STREAM CONNECT ID={} DESTINATION={} SILENT=false",
                session_id, destination
            );
            let response = self.send_command(&command)?;

            if response.starts_with("STREAM STATUS RESULT=OK") {
                // Extract port from response
                if let Some(port_str) = response.split("PORT=").nth(1) {
                    let port: u16 = port_str
                        .parse()
                        .map_err(|e| format!("Port parse failed: {e}"))?;

                    TcpStream::connect(format!("{}:{}", self.host, port))
                        .map_err(|e| format!("Stream connection failed: {e}"))
                } else {
                    Err("Failed to parse stream port".to_string())
                }
            } else {
                Err(format!("Stream connect failed: {}", response))
            }
        } else {
            Err("No active session".to_string())
        }
    }

    /// Accepts incoming connections
    pub fn accept_connection(&mut self) -> Result<TcpStream, String> {
        if let Some(session_id) = &self.session_id {
            let command = format!("STREAM ACCEPT ID={} SILENT=false", session_id);
            let response = self.send_command(&command)?;

            if response.starts_with("STREAM STATUS RESULT=OK") {
                // Extract port from response
                if let Some(port_str) = response.split("PORT=").nth(1) {
                    let port: u16 = port_str
                        .parse()
                        .map_err(|e| format!("Port parse failed: {e}"))?;

                    TcpStream::connect(format!("{}:{}", self.host, port))
                        .map_err(|e| format!("Stream connection failed: {e}"))
                } else {
                    Err("Failed to parse stream port".to_string())
                }
            } else {
                Err(format!("Stream accept failed: {}", response))
            }
        } else {
            Err("No active session".to_string())
        }
    }

    /// Gets the current session destination
    pub fn get_destination(&self) -> Option<String> {
        self.destination.clone()
    }

    /// Gets the current session ID
    pub fn get_session_id(&self) -> Option<String> {
        self.session_id.clone()
    }
}

impl Drop for I2PSamClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

/// I2P tunnel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I2PTunnelConfig {
    pub sam_host: String,
    pub sam_port: u16,
    pub session_id: String,
    pub destination: Option<String>,
    pub in_tunnel_count: u32,
    pub out_tunnel_count: u32,
}

impl Default for I2PTunnelConfig {
    fn default() -> Self {
        Self {
            sam_host: SAM_DEFAULT_HOST.to_string(),
            sam_port: SAM_DEFAULT_PORT,
            session_id: "soshal".to_string(),
            destination: None,
            in_tunnel_count: 2,
            out_tunnel_count: 2,
        }
    }
}

/// I2P tunnel manager
pub struct I2PTunnelManager {
    client: I2PSamClient,
    config: I2PTunnelConfig,
}

impl I2PTunnelManager {
    /// Creates a new I2P tunnel manager
    pub fn new(config: I2PTunnelConfig) -> Self {
        let client = I2PSamClient::new(config.sam_host.clone(), config.sam_port);
        Self { client, config }
    }

    /// Starts the I2P tunnel
    pub fn start(&mut self) -> Result<String, String> {
        self.client.connect()?;
        self.client.handshake()?;

        let dest = if let Some(ref dest) = self.config.destination {
            self.client
                .create_session(&self.config.session_id, Some(dest))?;
            dest.clone()
        } else {
            let generated = self.client.generate_destination()?;
            self.client.create_session(&self.config.session_id, None)?;
            generated
        };

        Ok(dest)
    }

    /// Stops the I2P tunnel
    pub fn stop(&mut self) -> Result<(), String> {
        self.client.disconnect()
    }

    /// Gets the client reference
    pub fn client(&mut self) -> &mut I2PSamClient {
        &mut self.client
    }
}

// ---------------------------------------------------------------------------
// Persistent session manager
// ---------------------------------------------------------------------------

/// Process-wide manager holding one SAM session for the app lifetime, so
/// P2P/streaming can both initiate and accept connections over i2p without
/// rebuilding the tunnel per call.
pub struct I2PSessionManager {
    tunnel: std::sync::Mutex<Option<I2PTunnelManager>>,
}

impl I2PSessionManager {
    pub fn new() -> Self {
        Self {
            tunnel: std::sync::Mutex::new(None),
        }
    }

    /// Starts (or restarts) the session. Pass the persistent destination from
    /// a previous run to keep the same address; `None` creates a transient
    /// destination for this run.
    pub fn start(&self, destination: Option<&str>) -> Result<String, String> {
        let mut guard = self.tunnel.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut t) = guard.take() {
            let _ = t.stop();
        }
        let mut tunnel = I2PTunnelManager::new(I2PTunnelConfig {
            destination: destination.map(|d| d.to_string()),
            ..I2PTunnelConfig::default()
        });
        let dest = tunnel.start()?;
        *guard = Some(tunnel);
        Ok(dest)
    }

    pub fn stop(&self) {
        if let Some(mut t) = self.tunnel.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = t.stop();
        }
    }

    pub fn is_running(&self) -> bool {
        self.tunnel
            .lock()
            .map(|g| g.as_ref().is_some())
            .unwrap_or(false)
    }

    pub fn destination(&self) -> Option<String> {
        self.tunnel
            .lock()
            .ok()
            .and_then(|mut g| g.as_mut().and_then(|t| t.client().get_destination()))
    }

    /// Opens an outbound stream to a remote i2p destination.
    pub fn connect_to_destination(&self, destination: &str) -> Result<std::net::TcpStream, String> {
        match self
            .tunnel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            Some(t) => t.client().connect_to_destination(destination),
            None => Err("i2p session not running".to_string()),
        }
    }

    /// Blocks until an inbound connection arrives on the session.
    pub fn accept_connection(&self) -> Result<std::net::TcpStream, String> {
        match self
            .tunnel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            Some(t) => t.client().accept_connection(),
            None => Err("i2p session not running".to_string()),
        }
    }
}

impl Default for I2PSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Hybrid PQC double-ratchet codec for i2p SAM streams. Every message on
/// the wire is `u32 BE length ‖ body`; the first body in each direction is
/// a ratchet handshake frame (session id + hybrid public key), everything
/// after is an encrypted ratchet frame.
///
/// The outbound side owns the session id (the destination hash); the
/// inbound side adopts the id from the handshake frame, so both ends key
/// the shared process-wide crypto store identically.
pub struct I2PStreamCodec {
    crypto: &'static PqcLinkCrypto,
    peer: String,
    context: String,
    handshake_sent: bool,
    handshake_done: bool,
}

/// Upper bound for a framed i2p message (handshake pk or ratchet frame).
const MAX_I2P_FRAME: usize = 128 * 1024;

fn write_raw_frame(stream: &mut TcpStream, body: &[u8]) -> Result<(), String> {
    if body.len() > MAX_I2P_FRAME {
        return Err("i2p frame too large".to_string());
    }
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .map_err(|e| format!("i2p frame write failed: {e}"))?;
    stream
        .write_all(body)
        .map_err(|e| format!("i2p frame write failed: {e}"))?;
    stream
        .flush()
        .map_err(|e| format!("i2p frame flush failed: {e}"))
}

fn read_raw_frame(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("i2p frame read failed: {e}"))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_I2P_FRAME {
        return Err("bad i2p frame length".to_string());
    }
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("i2p frame read failed: {e}"))?;
    Ok(body)
}

impl I2PStreamCodec {
    /// Outbound side: session keyed by the remote destination hash.
    pub fn new(peer: &str) -> Self {
        Self {
            crypto: &PQ_LINK_CRYPTO,
            peer: peer.to_string(),
            context: format!("i2p:{}", peer),
            handshake_sent: false,
            handshake_done: false,
        }
    }

    /// Outbound side: builds the first handshake frame (own hybrid pk).
    pub fn outbound_first(&mut self) -> Result<Vec<u8>, String> {
        let pk = self.crypto.begin_handshake(&self.peer, &self.context)?;
        self.handshake_sent = true;
        Ok(PqcLinkCrypto::handshake_frame(&self.peer, &pk))
    }

    /// Outbound side: consumes the responder's handshake reply, completing
    /// the session.
    pub fn outbound_finish(&mut self, reply: &[u8]) -> Result<(), String> {
        if !self.handshake_sent {
            return Err("outbound handshake not sent".to_string());
        }
        let (_sid, pk) = parse_handshake_frame(reply)?;
        self.crypto.complete_handshake(&self.peer, pk)?;
        self.handshake_done = true;
        Ok(())
    }

    /// Inbound side: consumes the initiator's handshake frame and returns
    /// the reply carrying our hybrid pk. State is keyed under
    /// `inbound:<sid>` so a same-process both-ends link cannot collide with
    /// the outbound side's session; the ratchet context keeps the raw
    /// session id so both ends derive identical keys.
    pub fn inbound_accept(&mut self, frame: &[u8]) -> Result<Vec<u8>, String> {
        let (sid, _) = parse_handshake_frame(frame)?;
        let context = format!("i2p:{}", sid);
        let state_key = format!("inbound:{}", sid);
        let (_key, own_pk) = self.crypto.accept_handshake(&context, &state_key, frame)?;
        self.peer = state_key;
        self.context = context;
        self.handshake_done = true;
        Ok(PqcLinkCrypto::handshake_frame(sid, &own_pk))
    }

    /// Inbound codec before the handshake (peer key filled on accept).
    pub fn new_inbound() -> Self {
        Self {
            crypto: &PQ_LINK_CRYPTO,
            peer: String::new(),
            context: "i2p:inbound".to_string(),
            handshake_sent: false,
            handshake_done: false,
        }
    }

    pub fn handshake_done(&self) -> bool {
        self.handshake_done
    }

    /// Encrypts a payload and writes it as a length-prefixed ratchet frame.
    pub fn write_encrypted(&self, stream: &mut TcpStream, payload: &[u8]) -> Result<(), String> {
        if !self.handshake_done {
            return Err("i2p ratchet handshake incomplete".to_string());
        }
        let frame = self.crypto.encrypt(&self.peer, &self.context, payload)?;
        write_raw_frame(stream, &frame)
    }

    /// Reads a length-prefixed ratchet frame and decrypts it.
    pub fn read_decrypted(&self, stream: &mut TcpStream) -> Result<Vec<u8>, String> {
        if !self.handshake_done {
            return Err("i2p ratchet handshake incomplete".to_string());
        }
        let frame = read_raw_frame(stream)?;
        self.crypto.decrypt(&self.peer, &self.context, &frame)
    }
}

#[cfg(test)]
mod codec_tests {
    use super::*;
    use crate::pqc_link::FRAME_TAG_HANDSHAKE;
    use std::net::TcpListener;
    use std::thread;

    fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = thread::spawn(move || listener.accept().expect("accept").0);
        let client = TcpStream::connect(addr).expect("connect");
        let server = server.join().expect("join");
        (client, server)
    }

    #[test]
    fn handshake_and_encrypted_roundtrip() {
        let (mut client, mut server) = connected_pair();
        let mut outbound = I2PStreamCodec::new("dest-hash-1");
        let mut inbound = I2PStreamCodec::new_inbound();

        let hello = outbound.outbound_first().unwrap();
        write_raw_frame(&mut client, &hello).unwrap();
        let inbound_frame = read_raw_frame(&mut server).unwrap();
        let reply = inbound.inbound_accept(&inbound_frame).unwrap();
        assert!(inbound.handshake_done());
        write_raw_frame(&mut server, &reply).unwrap();
        let reply_frame = read_raw_frame(&mut client).unwrap();
        outbound.outbound_finish(&reply_frame).unwrap();
        assert!(outbound.handshake_done());

        let payload = b"secret i2p payload";
        outbound.write_encrypted(&mut client, payload).unwrap();
        let plain = inbound.read_decrypted(&mut server).unwrap();
        assert_eq!(plain, payload);

        inbound.write_encrypted(&mut server, b"reply").unwrap();
        let plain = outbound.read_decrypted(&mut client).unwrap();
        assert_eq!(plain, b"reply");
    }

    #[test]
    fn encrypted_io_before_handshake_fails() {
        let (mut client, _server) = connected_pair();
        let outbound = I2PStreamCodec::new("dest-hash-2");
        assert!(outbound.write_encrypted(&mut client, b"x").is_err());
        assert!(outbound.read_decrypted(&mut client).is_err());
    }

    #[test]
    fn outbound_finish_before_send_fails() {
        let mut outbound = I2PStreamCodec::new("dest-hash-3");
        assert!(outbound.outbound_finish(&[0u8; 8]).is_err());
        let hello = outbound.outbound_first().unwrap();
        assert!(hello[0] == FRAME_TAG_HANDSHAKE);
        assert!(outbound.outbound_finish(&[0x99u8; 8]).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    struct MockBridge {
        listener: TcpListener,
        conn_replies: Vec<Vec<String>>,
    }

    impl MockBridge {
        fn bind(conn_replies: Vec<Vec<String>>) -> (Self, u16) {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock bridge");
            let port = listener.local_addr().expect("local addr").port();
            (
                Self {
                    listener,
                    conn_replies,
                },
                port,
            )
        }

        fn serve(self) -> thread::JoinHandle<()> {
            thread::spawn(move || {
                eprintln!("[mock] serving on {}", self.listener.local_addr().unwrap());
                for replies in self.conn_replies {
                    eprintln!("[mock] waiting accept");
                    let Ok((mut stream, _)) = self.listener.accept() else {
                        eprintln!("[mock] accept failed");
                        break;
                    };
                    eprintln!("[mock] accepted");
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    for reply in replies {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        let _ = stream.write_all(reply.as_bytes());
                        let _ = stream.write_all(b"\n");
                        let _ = stream.flush();
                    }
                }
            })
        }
    }

    fn mock_client(replies: &[&str]) -> (I2PSamClient, thread::JoinHandle<()>) {
        let list = replies.iter().map(|s| s.to_string()).collect();
        let (bridge, port) = MockBridge::bind(vec![list]);
        let handle = bridge.serve();
        let mut client = I2PSamClient::new("127.0.0.1".to_string(), port);
        client.connect().expect("connect");
        client.handshake().expect("handshake");
        client
            .create_session("test-session", Some("dest-x"))
            .expect("session");
        (client, handle)
    }

    #[test]
    fn test_client_creation() {
        let client = I2PSamClient::default_client();
        assert_eq!(client.host, SAM_DEFAULT_HOST);
        assert_eq!(client.port, SAM_DEFAULT_PORT);
    }

    #[test]
    fn test_tunnel_config_default() {
        let config = I2PTunnelConfig::default();
        assert_eq!(config.sam_host, SAM_DEFAULT_HOST);
        assert_eq!(config.sam_port, SAM_DEFAULT_PORT);
        assert_eq!(config.in_tunnel_count, 2);
        assert_eq!(config.out_tunnel_count, 2);
    }

    #[test]
    fn test_command_formatting() {
        let command = "HELLO VERSION=3.1 MIN=3.0 MAX=3.3";
        assert!(command.contains("HELLO"));
        assert!(command.contains("VERSION=3.1"));
    }

    #[test]
    fn stream_port_parse_failures() {
        let (mut client, handle) = mock_client(&[
            "HELLO REPLY VERSION=3.1",
            "SESSION STATUS RESULT=OK",
            "STREAM STATUS RESULT=OK PORT=notaport",
        ]);
        let err = client.connect_to_destination("remote-dest").unwrap_err();
        assert!(
            err.contains("Port parse failed"),
            "expected parse failure, got: {err}"
        );
        handle.join().expect("bridge thread");

        let (mut client, handle) = mock_client(&[
            "HELLO REPLY VERSION=3.1",
            "SESSION STATUS RESULT=OK",
            "STREAM STATUS RESULT=OK",
        ]);
        let err = client.connect_to_destination("remote-dest").unwrap_err();
        assert_eq!(err, "Failed to parse stream port");
        handle.join().expect("bridge thread");

        let (mut client, handle) = mock_client(&[
            "HELLO REPLY VERSION=3.1",
            "SESSION STATUS RESULT=OK",
            "STREAM STATUS RESULT=OK PORT=alsobad",
        ]);
        let err = client.accept_connection().unwrap_err();
        assert!(
            err.contains("Port parse failed"),
            "expected parse failure, got: {err}"
        );
        handle.join().expect("bridge thread");

        let (mut client, handle) = mock_client(&[
            "HELLO REPLY VERSION=3.1",
            "SESSION STATUS RESULT=OK",
            "STREAM STATUS RESULT=OK",
        ]);
        let err = client.accept_connection().unwrap_err();
        assert_eq!(err, "Failed to parse stream port");
        handle.join().expect("bridge thread");
    }

    #[test]
    fn create_session_transient_missing_destination_errors() {
        let list = vec![
            "HELLO REPLY VERSION=3.1".to_string(),
            "SESSION STATUS RESULT=OK".to_string(),
        ];
        let (bridge, port) = MockBridge::bind(vec![list]);
        let handle = bridge.serve();
        let mut client = I2PSamClient::new("127.0.0.1".to_string(), port);
        client.connect().expect("connect");
        client.handshake().expect("handshake");
        let result = client.create_session("test-session", None);
        assert!(
            result.is_err(),
            "transient session without DESTINATION= should error, got Ok"
        );
        handle.join().expect("bridge thread");
    }

    #[test]
    fn managers_start_and_restart() {
        let (bridge, port) = MockBridge::bind(vec![vec![
            "HELLO REPLY VERSION=3.1".to_string(),
            "SESSION STATUS RESULT=OK".to_string(),
        ]]);
        let handle = bridge.serve();
        let mut tunnel = I2PTunnelManager::new(I2PTunnelConfig {
            sam_host: "127.0.0.1".to_string(),
            sam_port: port,
            destination: Some("persistent-dest".to_string()),
            ..I2PTunnelConfig::default()
        });
        assert_eq!(tunnel.start().expect("tunnel start"), "persistent-dest");
        assert_eq!(tunnel.client().get_session_id().as_deref(), Some("soshal"));
        handle.join().expect("bridge thread");

        // I2PSessionManager::start builds its own config with the default
        // SAM port, so the mock must bind 127.0.0.1:SAM_DEFAULT_PORT.
        let listener =
            TcpListener::bind(("127.0.0.1", SAM_DEFAULT_PORT)).expect("bind default sam port");
        let (bridge, _port) = (
            MockBridge {
                listener,
                conn_replies: vec![
                    vec![
                        "HELLO REPLY VERSION=3.1".to_string(),
                        "DEST REPLY DEST=gen-1".to_string(),
                        "SESSION STATUS RESULT=OK DESTINATION=trans-1".to_string(),
                    ],
                    vec![
                        "HELLO REPLY VERSION=3.1".to_string(),
                        "DEST REPLY DEST=gen-2".to_string(),
                        "SESSION STATUS RESULT=OK DESTINATION=trans-2".to_string(),
                    ],
                ],
            },
            0,
        );
        let handle = bridge.serve();
        let manager = I2PSessionManager::new();
        assert_eq!(manager.start(None).expect("first start"), "gen-1");
        assert_eq!(manager.start(None).expect("restart"), "gen-2");
        assert!(manager.is_running());
        assert_eq!(
            manager.destination().as_deref(),
            Some("trans-2"),
            "running tunnel arm should return current transient destination"
        );
        handle.join().expect("bridge thread");
    }
}
