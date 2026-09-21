//! I2P SAM V3 client implementation for anonymous networking.

use crate::pqc_link::{parse_handshake_frame, PqcLinkCrypto, PQ_LINK_CRYPTO};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

const SAM_DEFAULT_HOST: &str = "127.0.0.1";
const SAM_DEFAULT_PORT: u16 = 7656;
const SAM_VERSION: &str = "3.1";
const SAM_SIGNATURE_TYPE: &str = "7"; // Ed25519
const SAM_MAX_REPLY_LINE: u64 = 8192;
/// Extra bytes drained from a hostile oversized reply line before giving up
/// and resetting the session (bounds the drain; keeps the stream aligned).
const SAM_MAX_OVERSIZE_DRAIN: u64 = 64 * 1024;
const SAM_ENCRYPTION_TYPE: &str = "4"; // ECIES-X25519

/// I2P SAM V3 client for anonymous networking
pub struct I2PSamClient {
    host: String,
    port: u16,
    /// Persistent line reader over the control stream. Kept across
    /// `send_command` calls so bytes the router buffers past a reply line
    /// (read-ahead) are never discarded between commands — a per-call
    /// `BufReader` would drop them and desync the next reply.
    reader: Option<BufReader<TcpStream>>,
    /// Dup handle on the control socket. `shutdown` on the dup wakes any
    /// in-flight blocking read/write on the original immediately, letting a
    /// foreign thread unblock a stuck command (see `force_close`).
    socket: Option<Arc<TcpStream>>,
    session_id: Option<String>,
    destination: Option<String>,
}

impl I2PSamClient {
    /// Creates a new I2P SAM client
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            reader: None,
            socket: None,
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
        // SAM is a local daemon; only loopback addresses are accepted. A
        // remote host here would tunnel traffic through an untrusted SAM
        // endpoint (and could be a hostile relay capturing metadata).
        let addr = format!("{}:{}", self.host, self.port);
        let sock = std::net::ToSocketAddrs::to_socket_addrs(&addr)
            .map_err(|e| format!("SAM host resolve failed: {e}"))?
            .next()
            .ok_or("SAM host resolved to no address")?;
        if !sock.ip().is_loopback() {
            return Err("SAM bridge must be on a loopback address".to_string());
        }
        let stream = TcpStream::connect_timeout(&sock, Duration::from_secs(10))
            .map_err(|e| format!("SAM connection failed: {e}"))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set read timeout failed: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set write timeout failed: {e}"))?;

        self.reader = Some(BufReader::new(
            stream
                .try_clone()
                .map_err(|e| format!("SAM socket dup failed: {e}"))?,
        ));
        self.socket = Some(Arc::new(stream));
        Ok(())
    }

    /// Shuts the control socket down without a SESSION CLOSE round-trip.
    /// Safe to call from another thread: `shutdown` on the dup fd wakes any
    /// in-flight blocking read/write on the original immediately.
    pub fn force_close(&self) {
        if let Some(s) = &self.socket {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Returns a handle to the control socket dup (for cross-thread unblock).
    pub fn socket_handle(&self) -> Option<Arc<TcpStream>> {
        self.socket.clone()
    }

    /// Disconnects from the SAM bridge
    pub fn disconnect(&mut self) -> Result<(), String> {
        if let Some(session_id) = &self.session_id {
            self.send_command(&format!("SESSION CLOSE STYLE=STREAM ID={}", session_id))?;
        }

        if let Some(reader) = self.reader.take() {
            reader
                .get_ref()
                .shutdown(std::net::Shutdown::Both)
                .map_err(|e| format!("Stream shutdown failed: {e}"))?;
        }

        self.session_id = None;
        self.destination = None;
        Ok(())
    }

    /// Sends a SAM command and reads the response
    fn send_command(&mut self, command: &str) -> Result<String, String> {
        let reader = self.reader.as_mut().ok_or("Not connected to SAM bridge")?;

        reader
            .get_mut()
            .write_all(command.as_bytes())
            .map_err(|e| format!("Command send failed: {e}"))?;
        reader
            .get_mut()
            .write_all(b"\n")
            .map_err(|e| format!("Newline send failed: {e}"))?;
        reader
            .get_mut()
            .flush()
            .map_err(|e| format!("Flush failed: {e}"))?;

        // Read one reply line from the PERSISTENT reader. `by_ref().take()`
        // caps the line (hostile router cannot grow memory without bound)
        // but does not consume the BufReader's read-ahead buffer, so bytes
        // the router pipelined after this reply survive for the next call.
        let mut response = String::new();
        {
            let mut limited = reader.by_ref().take(SAM_MAX_REPLY_LINE + 1);
            limited
                .read_line(&mut response)
                .map_err(|e| format!("Response read failed: {e}"))?;
        }
        if response.len() as u64 >= SAM_MAX_REPLY_LINE {
            // A hostile/glitchy router sent a reply longer than the cap. The
            // `take()` above consumed exactly SAM_MAX_REPLY_LINE+1 bytes, so
            // any remainder of the giant line may still be in the socket —
            // without draining it the NEXT command would read garbage from
            // mid-line (protocol desync). Scan the buffered remainder with a
            // zero-timeout read: whatever's already queued gets consumed up to
            // the newline (bounded); a WouldBlock means there's nothing left,
            // so the stream is already aligned again.
            // Note: Duration::ZERO would DISABLE the timeout (infinite block)
            // on Linux — use a small nonzero bound so a router that stalls
            // mid-line doesn't hold us for the full command timeout.
            let _ = reader
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(250)));
            let mut drained = 0u64;
            let mut byte = [0u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => break, // EOF — aligned
                    Ok(_) => {
                        drained += 1;
                        if byte[0] == b'\n' {
                            break; // end of oversized line: aligned
                        }
                        if drained > SAM_MAX_OVERSIZE_DRAIN {
                            self.force_close();
                            return Err(
                                "SAM reply line exceeds drain cap; session reset".to_string()
                            );
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        self.force_close();
                        return Err(format!("Oversized reply drain failed: {e}"));
                    }
                }
            }
            // Restore the normal command read timeout.
            let _ = reader
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(30)));
            return Err("SAM reply line too long".to_string());
        }

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

                    let addr = std::net::ToSocketAddrs::to_socket_addrs(&format!(
                        "{}:{}",
                        self.host, port
                    ))
                    .map_err(|e| format!("Stream addr resolve failed: {e}"))?
                    .next()
                    .ok_or("Stream address resolved to nothing")?;
                    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
                        .map_err(|e| format!("Stream connection failed: {e}"))?;
                    // Bound the data stream: without timeouts a silent peer
                    // stalls broadcast writes forever and its reader thread
                    // never exits (thread/fd leak).
                    stream
                        .set_read_timeout(Some(Duration::from_secs(30)))
                        .map_err(|e| format!("Set stream read timeout failed: {e}"))?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(30)))
                        .map_err(|e| format!("Set stream write timeout failed: {e}"))?;
                    Ok(stream)
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

                    let addr = std::net::ToSocketAddrs::to_socket_addrs(&format!(
                        "{}:{}",
                        self.host, port
                    ))
                    .map_err(|e| format!("Stream addr resolve failed: {e}"))?
                    .next()
                    .ok_or("Stream address resolved to nothing")?;
                    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
                        .map_err(|e| format!("Stream connection failed: {e}"))?;
                    stream
                        .set_read_timeout(Some(Duration::from_secs(30)))
                        .map_err(|e| format!("Set stream read timeout failed: {e}"))?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(30)))
                        .map_err(|e| format!("Set stream write timeout failed: {e}"))?;
                    Ok(stream)
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
        // Never block in Drop: force-close the control socket (SESSION CLOSE
        // round-trip could stall against an unresponsive router).
        self.force_close();
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

    /// Detaches the underlying SAM client so a session manager can serialize
    /// command I/O separately from lifecycle operations.
    pub fn into_client(self) -> I2PSamClient {
        self.client
    }
}

// ---------------------------------------------------------------------------
// Persistent session manager
// ---------------------------------------------------------------------------

/// Process-wide manager holding one SAM session for the app lifetime, so
/// P2P/streaming can both initiate and accept connections over i2p without
/// rebuilding the tunnel per call.
///
/// Lock discipline (single direction, no ABBA):
/// - Blocking STREAM CONNECT/ACCEPT command I/O serializes on `client` only.
/// - Lifecycle ops (`start`/`stop`/`is_running`/`destination`) touch only
///   `lifecycle`; `stop` unblocks an in-flight blocking op via a dup-fd
///   `shutdown` on the control socket, so it never waits behind an accept.
pub struct I2PSessionManager {
    /// The SAM control client. Held across blocking command I/O; lifecycle
    /// ops never acquire it while a command is in flight.
    client: std::sync::Mutex<Option<I2PSamClient>>,
    /// Lifecycle bookkeeping, independent of command I/O.
    lifecycle: std::sync::Mutex<I2PManagerLifecycle>,
    config: I2PTunnelConfig,
}

#[derive(Default)]
struct I2PManagerLifecycle {
    destination: Option<String>,
    session_id: Option<String>,
    running: bool,
    /// Dup of the SAM control socket: lets `stop()` unblock an in-flight
    /// blocking op without acquiring the `client` lock.
    control: Option<Arc<TcpStream>>,
}

impl I2PSessionManager {
    pub fn new() -> Self {
        Self::with_config(I2PTunnelConfig::default())
    }

    pub fn with_config(config: I2PTunnelConfig) -> Self {
        Self {
            client: std::sync::Mutex::new(None),
            lifecycle: std::sync::Mutex::new(I2PManagerLifecycle::default()),
            config,
        }
    }

    /// Starts (or restarts) the session. Pass the persistent destination from
    /// a previous run to keep the same address; `None` creates a transient
    /// destination for this run.
    pub fn start(&self, destination: Option<&str>) -> Result<String, String> {
        let mut config = self.config.clone();
        if let Some(d) = destination {
            config.destination = Some(d.to_string());
        }
        // Blocking setup runs on a local tunnel — no lock is held while the
        // SAM bridge handshakes, so concurrent connect/accept aren't stalled.
        let mut tunnel = I2PTunnelManager::new(config);
        let dest = tunnel.start()?;
        let client = tunnel.into_client();
        {
            let mut l = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
            // Addressable destination = the session destination negotiated
            // with the SAM bridge (transient or fixed), not the generated
            // keypair's pubkey. This is what relay peers connect to.
            l.destination = client.get_destination();
            l.session_id = client.get_session_id();
            l.running = true;
            l.control = client.socket_handle();
            drop(l);
        }
        *self.client.lock().unwrap_or_else(|e| e.into_inner()) = Some(client);
        Ok(dest)
    }

    pub fn stop(&self) {
        // 1. Unblock any in-flight blocking op right now: shutdown on the dup
        //    fd wakes the blocked read, the op returns Err, and the client
        //    lock frees. This is what makes relay shutdown fast even while
        //    the listener sits in STREAM ACCEPT.
        if let Some(sock) = self
            .lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .control
            .as_ref()
        {
            let _ = sock.shutdown(std::net::Shutdown::Both);
        }
        // 2. Now the client lock is free — take and drop the client.
        if let Some(client) = self.client.lock().unwrap_or_else(|e| e.into_inner()).take() {
            client.force_close();
        }
        // 3. Clear lifecycle state.
        let mut l = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        l.running = false;
        l.destination = None;
        l.session_id = None;
        l.control = None;
    }

    pub fn is_running(&self) -> bool {
        self.lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .running
    }

    pub fn destination(&self) -> Option<String> {
        self.lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .destination
            .clone()
    }

    /// Opens an outbound stream to a remote i2p destination. Blocks while the
    /// SAM bridge establishes the tunnel, but only serializes with other
    /// command I/O — lifecycle ops (`stop`/`start`) are never blocked.
    pub fn connect_to_destination(&self, destination: &str) -> Result<std::net::TcpStream, String> {
        let mut guard = self.client.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(c) => c.connect_to_destination(destination),
            None => Err("i2p session not running".to_string()),
        }
    }

    /// Blocks until an inbound connection arrives on the session. Same lock
    /// discipline as `connect_to_destination`: command I/O only.
    pub fn accept_connection(&self) -> Result<std::net::TcpStream, String> {
        let mut guard = self.client.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(c) => c.accept_connection(),
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
    use std::time::Instant;

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

        // I2PSessionManager uses with_config so test can use an ephemeral port.
        let (bridge, mgr_port) = MockBridge::bind(vec![
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
        ]);
        let handle = bridge.serve();
        let manager = I2PSessionManager::with_config(I2PTunnelConfig {
            sam_port: mgr_port,
            ..I2PTunnelConfig::default()
        });
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

    #[test]
    fn pipelined_reply_bytes_survive_between_commands() {
        // A well-behaved router never sends unsolicited control lines, but a
        // hostile/glitchy one may pipeline several reply lines while the
        // client is between commands. A per-call BufReader would swallow the
        // read-ahead bytes after the first reply line; the persistent reader
        // must deliver them to the next command.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            reader.read_line(&mut line).expect("read cmd1");
            // Reply with TWO lines in a single segment (forces read-ahead
            // past the first reply line).
            stream
                .write_all(b"HELLO REPLY VERSION=3.1\nSESSION STATUS RESULT=OK\n")
                .expect("write pipelined replies");
            line.clear();
            reader.read_line(&mut line).expect("read cmd2");
            // Unblock a broken (per-call-BufReader) client whose second read
            // drained the socket: it would observe this line instead.
            let _ = stream.write_all(b"SESSION STATUS RESULT=DUPLICATED\n");
            let _ = stream.flush();
        });

        let mut client = I2PSamClient::new("127.0.0.1".to_string(), port);
        client.connect().expect("connect");
        let r1 = client
            .send_command("HELLO VERSION=3.1 MIN=3.0 MAX=3.3")
            .expect("cmd1");
        assert_eq!(r1, "HELLO REPLY VERSION=3.1");
        let r2 = client
            .send_command("SESSION CREATE STYLE=STREAM ID=s")
            .expect("cmd2");
        assert_eq!(r2, "SESSION STATUS RESULT=OK");
        handle.join().expect("bridge thread");
    }

    #[test]
    fn overlong_reply_line_is_rejected() {
        // Regression guard: the reply-line cap must reject a hostile router
        // line without hanging or unbounded memory growth.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            reader.read_line(&mut line).expect("read cmd");
            let huge = "x".repeat(SAM_MAX_REPLY_LINE as usize);
            let _ = stream.write_all(huge.as_bytes());
            let _ = stream.write_all(b"\n");
            let _ = stream.flush();
            // Read until the client closes.
            let mut sink = String::new();
            let _ = reader.read_line(&mut sink);
        });

        let mut client = I2PSamClient::new("127.0.0.1".to_string(), port);
        client.connect().expect("connect");
        let err = client
            .send_command("HELLO VERSION=3.1 MIN=3.0 MAX=3.3")
            .unwrap_err();
        assert!(
            err.contains("too long"),
            "expected too-long error, got: {err}"
        );
        // Close our side so the bridge's trailing read_line sees EOF.
        drop(client);
        handle.join().expect("bridge thread");
    }

    /// Regression: an oversized reply line used to leave the remainder of the
    /// line in the persistent reader, so the NEXT command on the same session
    /// read garbage from mid-line (protocol desync). The drain must keep the
    /// stream aligned.
    #[test]
    fn overlong_reply_does_not_desync_next_command() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            // cmd1 -> hostile oversized line (cap + extra body + newline).
            let mut line = String::new();
            reader.read_line(&mut line).expect("read cmd1");
            let huge = "x".repeat(SAM_MAX_REPLY_LINE as usize);
            let _ = stream.write_all(huge.as_bytes());
            let _ = stream.write_all(&"y".repeat(1024).into_bytes());
            let _ = stream.write_all(b"\n");
            let _ = stream.flush();
            // cmd2 on the SAME session -> must still read a clean reply.
            line.clear();
            reader.read_line(&mut line).expect("read cmd2");
            let _ = stream.write_all(b"SESSION STATUS RESULT=OK\n");
            let _ = stream.flush();
        });

        let mut client = I2PSamClient::new("127.0.0.1".to_string(), port);
        client.connect().expect("connect");
        // First command gets the oversized reply.
        let err = client
            .create_session("aligned-session", Some("dest-aligned"))
            .unwrap_err();
        assert!(err.contains("too long"), "expected too-long, got: {err}");
        // The persistent reader must now be aligned: the next command gets
        // the normal reply, not a garbage response.
        let r = client
            .send_command("STREAM CONNECT ID=aligned-session DESTINATION=x SILENT=false")
            .expect("second command must parse cleanly");
        assert_eq!(r, "SESSION STATUS RESULT=OK");
        drop(client);
        handle.join().expect("bridge thread");
    }

    /// Regression: `stop()` must unblock an in-flight blocking `accept` and
    /// return promptly instead of waiting out the SAM bridge's 30 s read
    /// timeout (which wedged relay shutdown).
    #[test]
    fn manager_stop_unblocks_inflight_accept() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            // HELLO
            reader.read_line(&mut line).expect("read hello");
            let _ = stream.write_all(b"HELLO REPLY VERSION=3.1\n");
            line.clear();
            // DEST GENERATE
            reader.read_line(&mut line).expect("read dest");
            let _ = stream.write_all(b"DEST REPLY DEST=gen-x\n");
            line.clear();
            // SESSION CREATE (transient)
            reader.read_line(&mut line).expect("read session");
            let _ = stream.write_all(b"SESSION STATUS RESULT=OK DESTINATION=trans-x\n");
            line.clear();
            // STREAM ACCEPT — never reply; holds the accept open until the
            // client's dup-fd shutdown wakes this read (EOF/reset).
            let _ = reader.read_line(&mut line);
        });

        let manager = std::sync::Arc::new(I2PSessionManager::with_config(I2PTunnelConfig {
            sam_port: port,
            ..I2PTunnelConfig::default()
        }));
        manager.start(None).expect("start");
        // Listener thread parks in accept_connection (server never replies).
        let session = std::sync::Arc::clone(&manager);
        let t = thread::spawn(move || {
            let _ = session.accept_connection();
        });
        thread::sleep(Duration::from_millis(200));
        let t0 = Instant::now();
        manager.stop();
        let elapsed = t0.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "stop took {elapsed:?} — in-flight accept was not unblocked"
        );
        assert!(!manager.is_running());
        let _ = t.join();
        handle.join().expect("bridge thread");
    }
}
