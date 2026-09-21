//! I2P SAM stream backend: peer exchange over direct SAM stream sockets.
//!
//! Frames are length-prefixed (u32 LE) over each TcpStream. Peers are
//! addressed by I2P destination hash.

use crate::backends::{BackendKind, MeshBackend};
use crate::envelope::MAX_ENVELOPE_BYTES;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Inbound queue cap; oldest frames dropped past it.
const RECEIVED_CAP: usize = 4096;
/// Cap on concurrent inbound reader threads; connections beyond it are
/// dropped (bounds per-connection thread/fd DoS).
const MAX_INBOUND_STREAMS: usize = 64;
/// Cap on outbound peer connections; oldest idle connections are closed and
/// evicted when full so the map cannot grow without bound.
const MAX_OUTBOUND_PEERS: usize = 32;

pub struct I2pBackend {
    session: Option<Arc<soshal_network_core::i2p_sam::I2PSessionManager>>,
    destination: String,
    peers: Mutex<HashMap<String, (TcpStream, Instant)>>,
    listener: Option<std::thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
    received: Arc<Mutex<VecDeque<Vec<u8>>>>,
    started: bool,
}

impl I2pBackend {
    pub fn new() -> Self {
        Self {
            session: None,
            destination: String::new(),
            peers: Mutex::new(HashMap::new()),
            listener: None,
            running: Arc::new(AtomicBool::new(false)),
            received: Arc::new(Mutex::new(VecDeque::new())),
            started: false,
        }
    }

    /// Opens an outbound SAM stream to a destination and tracks it as a peer.
    pub fn connect(&mut self, destination: &str) -> Result<(), String> {
        if !self.started {
            return Err("i2p backend not started".to_string());
        }
        let session = self.session.as_ref().ok_or("i2p session missing")?;
        let stream = session.connect_to_destination(destination)?;
        let reader = stream
            .try_clone()
            .map_err(|e| format!("stream clone failed: {e}"))?;
        {
            let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
            // Cap outbound peers: evict oldest idle connection when full.
            if peers.len() >= MAX_OUTBOUND_PEERS {
                if let Some(oldest_key) = peers
                    .iter()
                    .min_by_key(|(_, (_, ts))| *ts)
                    .map(|(k, _)| k.clone())
                {
                    peers.remove(&oldest_key);
                }
            }
            // Reconnecting to an already-known destination must close the old
            // socket — otherwise the old reader thread never sees EOF and the
            // fd leaks. shutdown() wakes it with an error right away.
            if let Some((old, _)) = peers.remove(destination) {
                let _ = old.shutdown(std::net::Shutdown::Both);
            }
            peers.insert(destination.to_string(), (stream, Instant::now()));
        }
        let received = self.received.clone();
        std::thread::spawn(move || reader_loop(reader, received));
        Ok(())
    }
}

impl Default for I2pBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshBackend for I2pBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::I2p
    }
    fn start(&mut self) -> Result<(), String> {
        let session = Arc::new(soshal_network_core::i2p_sam::I2PSessionManager::new());
        session.start(None)?;
        self.destination = session.destination().unwrap_or_default();
        self.running.store(true, Ordering::Relaxed);
        self.started = true;
        let running = self.running.clone();
        let received = self.received.clone();
        let session_thread = session.clone();
        let active_inbound = Arc::new(AtomicUsize::new(0));
        self.listener = Some(std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                match session_thread.accept_connection() {
                    Ok(stream) => {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
                        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
                        if active_inbound.load(Ordering::Relaxed) >= MAX_INBOUND_STREAMS {
                            drop(stream); // reject beyond cap
                            continue;
                        }
                        active_inbound.fetch_add(1, Ordering::Relaxed);
                        let received = received.clone();
                        let active_inbound = active_inbound.clone();
                        std::thread::spawn(move || {
                            reader_loop(stream, received);
                            active_inbound.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
                }
            }
        }));
        self.session = Some(session);
        Ok(())
    }
    fn stop(&mut self) {
        self.started = false;
        self.running.store(false, Ordering::Relaxed);
        if let Some(session) = &self.session {
            session.stop();
        }
        // Join the listener thread: `session.stop()` unblocks an in-flight
        // STREAM ACCEPT (dup-fd shutdown), so this returns promptly instead of
        // waiting out the SAM bridge's read timeout.
        if let Some(handle) = self.listener.take() {
            let _ = handle.join();
        }
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    fn running(&self) -> bool {
        self.started
    }
    fn broadcast(&mut self, payload: Vec<u8>) -> Result<usize, String> {
        if !self.started {
            return Ok(0);
        }
        if payload.len() > MAX_ENVELOPE_BYTES {
            return Err("payload exceeds mesh cap".to_string());
        }
        let frame = encode_frame(&payload);
        let snapshot: Vec<(String, TcpStream)> = {
            let peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
            peers
                .iter()
                .filter_map(|(dest, (stream, _))| {
                    stream.try_clone().ok().map(|s| (dest.clone(), s))
                })
                .collect()
        };
        let mut failed = Vec::new();
        for (dest, mut stream) in snapshot {
            let _ = stream
                .write_all(&frame)
                .map_err(|_| failed.push(dest.clone()));
        }
        if !failed.is_empty() {
            let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
            for dest in &failed {
                peers.remove(dest);
            }
        }
        Ok(self.peers.lock().unwrap_or_else(|e| e.into_inner()).len())
    }
    fn recv(&mut self) -> Vec<Vec<u8>> {
        let mut queue = self.received.lock().unwrap_or_else(|e| e.into_inner());
        queue.drain(..).collect()
    }
    fn peers(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .peers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }
    fn peers_count(&self) -> usize {
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Drop for I2pBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(session) = &self.session {
            session.stop();
        }
        if let Some(handle) = self.listener.take() {
            let _ = handle.join();
        }
    }
}

/// Encodes a payload as u32 LE length prefix + bytes.
fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Reads one frame from a stream.
fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("read frame len failed: {e}"))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_ENVELOPE_BYTES {
        return Err(format!("frame length {len} exceeds mesh cap"));
    }
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("read frame payload failed: {e}"))?;
    Ok(payload)
}

/// Drains frames from a stream into the shared queue until it errors.
fn reader_loop(stream: TcpStream, received: Arc<Mutex<VecDeque<Vec<u8>>>>) {
    let mut stream = stream;
    while let Ok(frame) = read_frame(&mut stream) {
        let mut queue = received.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() >= RECEIVED_CAP {
            queue.pop_front();
        }
        queue.push_back(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn test_encode_and_read_frame() {
        let payload = b"hello i2p".to_vec();
        let frame = encode_frame(&payload);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let mut client = TcpStream::connect(addr).expect("connect");
        let (mut server, _) = listener.accept().expect("accept");
        client.write_all(&frame).expect("write frame");
        let read = read_frame(&mut server).expect("read frame");
        assert_eq!(read, payload);
    }

    #[test]
    fn test_read_frame_rejects_bad_len() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let mut client = TcpStream::connect(addr).expect("connect");
        let (mut server, _) = listener.accept().expect("accept");
        client.write_all(&[0xff; 4]).expect("write len");
        let err = read_frame(&mut server).expect_err("expected err");
        assert!(err.contains("exceeds"), "got: {err}");
    }
}
