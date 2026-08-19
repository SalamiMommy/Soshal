//! Hybrid PQC double-ratchet link encryption shared by network transports.
//!
//! Wraps the crypto-core v3 double ratchet (X25519 + ML-KEM-768 hybrid KEM)
//! in transport framing. Two message kinds on the wire:
//!
//! - **Handshake**: `0x00 ‖ <own hybrid pk hex ascii>` — plaintext key
//!   exchange that bootstraps a session. After the exchange both sides hold
//!   a ratchet state; the first ratchet frame performs the KEM root step.
//! - **Ratchet frame**: `0x01 ‖ <u16 BE header json len> ‖ <header json> ‖
//!   <ciphertext bytes>` — every payload after the handshake.
//!
//! Per-peer session state lives in memory only: link sessions are ephemeral
//! and re-established per connection. Binary payloads are base64-wrapped
//! before encryption (the ratchet plaintext is a JSON string).
//!
//! Transports keep their own peer identities and contexts (e.g.
//! `reticulum:<addr-hex>`, `i2p:<destination>`, `freenet:<peer>`), so one
//! process-wide instance can serve all three without key collisions.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use soshal_crypto_core::pqc_ratchet::{
    decrypt_ratchet, encrypt_ratchet, init_state, HeaderOutput, RatchetOutput,
};
use soshal_pqc_core::hybrid::hybrid_keygen;

/// Frame tag for the plaintext hybrid-public-key handshake.
pub const FRAME_TAG_HANDSHAKE: u8 = 0x00;
/// Frame tag for an encrypted ratchet frame.
pub const FRAME_TAG_RATCHET: u8 = 0x01;

/// Largest accepted ratchet header JSON (bounds forged frame allocations).
pub const MAX_LINK_HEADER_JSON: usize = 8 * 1024;
/// Largest accepted link frame (header + ciphertext + framing slack).
pub const MAX_LINK_FRAME: usize = 96 * 1024;
/// Plaintext cap: base64 inflates 4/3, and the ratchet ciphertext cap is
/// 64 KiB, so 48 KiB keeps every frame inside the cap.
pub const MAX_LINK_PAYLOAD: usize = 48 * 1024;

/// Per-peer ratchet session state kept in memory for the link lifetime.
pub struct PqcLinkCrypto {
    states: Mutex<HashMap<String, RatchetOutput>>,
}

impl Default for PqcLinkCrypto {
    fn default() -> Self {
        Self::new()
    }
}

impl PqcLinkCrypto {
    pub fn new() -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, peer: &str) -> Result<RatchetOutput, String> {
        self.states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(peer)
            .cloned()
            .ok_or("no ratchet session for peer".to_string())
    }

    fn put(&self, peer: &str, state: RatchetOutput) {
        self.states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(peer.to_string(), state);
    }

    /// Initiator side of a link handshake: generates the session keypair and
    /// returns the own hybrid public key to send to the peer. The session is
    /// registered with an empty peer key; `complete_handshake` fills it.
    pub fn begin_handshake(&self, peer: &str, context: &str) -> Result<String, String> {
        if self.has_session(peer) {
            return Err("ratchet session already exists for peer".to_string());
        }
        let (pk, sk) = hybrid_keygen().map_err(|e| format!("hybrid keygen failed: {e}"))?;
        let state = init_state("", context, &sk, &pk);
        self.put(peer, state);
        Ok(pk)
    }

    /// Initiator side: records the peer's handshake public key. After this
    /// call the first `encrypt` performs the KEM root step.
    pub fn complete_handshake(&self, peer: &str, peer_pk: &str) -> Result<(), String> {
        if peer_pk.trim().is_empty() {
            return Err("empty peer public key".to_string());
        }
        let mut state = self.get(peer)?;
        state.peer_pk = peer_pk.to_string();
        self.put(peer, state);
        Ok(())
    }

    /// Responder side: consumes an inbound handshake frame carrying the
    /// initiator's public key, creates the session, and returns the own
    /// public key to send back. The frame's session id selects the state
    /// key (an outbound destination hash for i2p, address hex elsewhere),
    /// so both ends of a connection agree on the key without a separate
    /// channel. `state_key` may namespace the responder's store (e.g.
    /// `inbound:<sid>`) to avoid collisions when both ends run in one
    /// process. Returns `(state_key, own_pk)`.
    pub fn accept_handshake(
        &self,
        context: &str,
        state_key: &str,
        frame: &[u8],
    ) -> Result<(String, String), String> {
        let (_sid, peer_pk) = parse_handshake_frame(frame)?;
        if peer_pk.trim().is_empty() {
            return Err("empty peer public key".to_string());
        }
        if self.has_session(state_key) {
            return Err("ratchet session already exists for peer".to_string());
        }
        let (pk, sk) = hybrid_keygen().map_err(|e| format!("hybrid keygen failed: {e}"))?;
        let state = init_state(peer_pk, context, &sk, &pk);
        self.put(state_key, state);
        Ok((state_key.to_string(), pk))
    }

    /// Responder side, packet transport variant: same as `accept_handshake`
    /// but the initiator key arrives in a transport packet payload, not a
    /// framed message.
    pub fn accept_handshake_pk(
        &self,
        peer: &str,
        context: &str,
        peer_pk: &str,
    ) -> Result<String, String> {
        if peer_pk.trim().is_empty() {
            return Err("empty peer public key".to_string());
        }
        if self.has_session(peer) {
            return Err("ratchet session already exists for peer".to_string());
        }
        let (pk, sk) = hybrid_keygen().map_err(|e| format!("hybrid keygen failed: {e}"))?;
        let state = init_state(peer_pk, context, &sk, &pk);
        self.put(peer, state);
        Ok(pk)
    }

    /// Builds a handshake frame carrying the given session id and hybrid
    /// public key.
    pub fn handshake_frame(sid: &str, pk_hex: &str) -> Vec<u8> {
        let mut frame = Vec::with_capacity(1 + 4 + sid.len() + pk_hex.len());
        frame.push(FRAME_TAG_HANDSHAKE);
        frame.extend_from_slice(&(sid.len() as u32).to_be_bytes());
        frame.extend_from_slice(sid.as_bytes());
        frame.extend_from_slice(pk_hex.as_bytes());
        frame
    }

    pub fn has_session(&self, peer: &str) -> bool {
        self.states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(peer)
    }

    /// Own live hybrid public key for a peer (sent in headers, used by the
    /// peer for its next encapsulate).
    pub fn own_pk(&self, peer: &str) -> Option<String> {
        self.get(peer).ok().map(|s| s.current_pk.clone())
    }

    /// Idempotent session bootstrap: creates the session if missing, then
    /// pins the peer's public key. Safe to call before every encrypt.
    pub fn ensure_session(&self, peer: &str, context: &str, peer_pk: &str) -> Result<(), String> {
        if !self.has_session(peer) {
            self.begin_handshake(peer, context)?;
        }
        self.complete_handshake(peer, peer_pk)
    }

    /// Encrypts a payload into a ratchet frame. Requires the handshake to be
    /// complete (peer key known).
    pub fn encrypt(&self, peer: &str, _context: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        if payload.len() > MAX_LINK_PAYLOAD {
            return Err("payload too large for ratchet link frame".to_string());
        }
        let state = self.get(peer)?;
        if state.peer_pk.is_empty() {
            return Err("ratchet handshake incomplete: no peer public key".to_string());
        }
        let b64 = B64.encode(payload);
        let (new_state, header, ct_hex) = encrypt_ratchet(&state, &b64)?;
        self.put(peer, new_state);

        let header_json = serde_json::to_string(&header)
            .map_err(|e| format!("header serialization failed: {e}"))?;
        if header_json.len() > MAX_LINK_HEADER_JSON {
            return Err("ratchet header too large".to_string());
        }

        // The ratchet ciphertext is base64 text; ship it verbatim.
        let mut frame = Vec::with_capacity(1 + 2 + header_json.len() + ct_hex.len());
        frame.push(FRAME_TAG_RATCHET);
        frame.extend_from_slice(&(header_json.len() as u16).to_be_bytes());
        frame.extend_from_slice(header_json.as_bytes());
        frame.extend_from_slice(ct_hex.as_bytes());
        Ok(frame)
    }

    /// Decrypts a ratchet frame produced by `encrypt`.
    pub fn decrypt(&self, peer: &str, _context: &str, frame: &[u8]) -> Result<Vec<u8>, String> {
        if frame.len() < 3 || frame[0] != FRAME_TAG_RATCHET {
            return Err("bad ratchet frame".to_string());
        }
        if frame.len() > MAX_LINK_FRAME {
            return Err("ratchet frame too large".to_string());
        }
        let hlen = u16::from_be_bytes([frame[1], frame[2]]) as usize;
        if hlen > MAX_LINK_HEADER_JSON || frame.len() < 3 + hlen + 1 {
            return Err("bad ratchet header length".to_string());
        }
        let header_json =
            std::str::from_utf8(&frame[3..3 + hlen]).map_err(|_| "header not utf8".to_string())?;
        let header: HeaderOutput =
            serde_json::from_str(header_json).map_err(|_| "bad ratchet header json".to_string())?;
        let ct = std::str::from_utf8(&frame[3 + hlen..])
            .map_err(|_| "ciphertext not utf8".to_string())?
            .to_string();

        let state = self.get(peer)?;
        let (new_state, plaintext) = decrypt_ratchet(&state, &header, &ct)?;
        self.put(peer, new_state);

        B64.decode(plaintext)
            .map_err(|_| "bad plaintext base64".to_string())
    }

    /// Drops the session for a peer (link closed / pruned).
    pub fn remove(&self, peer: &str) {
        self.states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(peer);
    }
}

/// Parses `0x00 ‖ <u32 BE sid len> ‖ <sid> ‖ <pk ascii>` handshake frames.
pub(crate) fn parse_handshake_frame(frame: &[u8]) -> Result<(&str, &str), String> {
    if frame.len() < 5 || frame[0] != FRAME_TAG_HANDSHAKE {
        return Err("bad handshake frame".to_string());
    }
    let sid_len = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;
    if frame.len() < 5 + sid_len + 1 {
        return Err("bad handshake frame".to_string());
    }
    let sid = std::str::from_utf8(&frame[5..5 + sid_len]).map_err(|_| "handshake sid not ascii")?;
    let pk = std::str::from_utf8(&frame[5 + sid_len..]).map_err(|_| "handshake pk not ascii")?;
    Ok((sid, pk))
}

/// Process-wide link crypto instance shared by the Reticulum, i2p and
/// Freenet transports. Peer ids include the transport name, so sessions
/// never collide.
pub static PQ_LINK_CRYPTO: LazyLock<PqcLinkCrypto> = LazyLock::new(PqcLinkCrypto::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_and_roundtrip() {
        let a = PqcLinkCrypto::new();
        let b = PqcLinkCrypto::new();
        let ctx = "test:peer1";

        let pk_a = a.begin_handshake("peer1", ctx).unwrap();
        let frame_a = PqcLinkCrypto::handshake_frame("peer1", &pk_a);
        let (sid_b, pk_b) = b.accept_handshake(ctx, "peer1", &frame_a).unwrap();
        assert_eq!(sid_b, "peer1");
        a.complete_handshake("peer1", &pk_b).unwrap();

        assert!(a.has_session("peer1"));
        assert!(b.has_session("peer1"));

        let payload = b"hello over the hybrid ratchet";
        let cipher = a.encrypt("peer1", ctx, payload).unwrap();
        assert_ne!(cipher, payload);
        let plain = b.decrypt("peer1", ctx, &cipher).unwrap();
        assert_eq!(plain, payload);
    }

    #[test]
    fn multi_message_chain() {
        let a = PqcLinkCrypto::new();
        let b = PqcLinkCrypto::new();
        let ctx = "test:peer2";
        let pk_a = a.begin_handshake("peer2", ctx).unwrap();
        let (_sid_b, pk_b) = b
            .accept_handshake(
                ctx,
                "peer2",
                &PqcLinkCrypto::handshake_frame("peer2", &pk_a),
            )
            .unwrap();
        a.complete_handshake("peer2", &pk_b).unwrap();

        for i in 0..5 {
            let msg = format!("message {i}");
            let cipher = a.encrypt("peer2", ctx, msg.as_bytes()).unwrap();
            let plain = b.decrypt("peer2", ctx, &cipher).unwrap();
            assert_eq!(plain, msg.as_bytes());
        }
    }

    #[test]
    fn out_of_order_delivery() {
        let a = PqcLinkCrypto::new();
        let b = PqcLinkCrypto::new();
        let ctx = "test:peer3";
        let pk_a = a.begin_handshake("peer3", ctx).unwrap();
        let (_sid_b, pk_b) = b
            .accept_handshake(
                ctx,
                "peer3",
                &PqcLinkCrypto::handshake_frame("peer3", &pk_a),
            )
            .unwrap();
        a.complete_handshake("peer3", &pk_b).unwrap();

        let f1 = a.encrypt("peer3", ctx, b"first").unwrap();
        let f2 = a.encrypt("peer3", ctx, b"second").unwrap();
        let f3 = a.encrypt("peer3", ctx, b"third").unwrap();

        assert_eq!(b.decrypt("peer3", ctx, &f1).unwrap(), b"first");
        assert_eq!(b.decrypt("peer3", ctx, &f3).unwrap(), b"third");
        assert_eq!(b.decrypt("peer3", ctx, &f2).unwrap(), b"second");
    }

    #[test]
    fn replay_and_tamper_rejected() {
        let a = PqcLinkCrypto::new();
        let b = PqcLinkCrypto::new();
        let ctx = "test:peer4";
        let pk_a = a.begin_handshake("peer4", ctx).unwrap();
        let (_sid_b, pk_b) = b
            .accept_handshake(
                ctx,
                "peer4",
                &PqcLinkCrypto::handshake_frame("peer4", &pk_a),
            )
            .unwrap();
        a.complete_handshake("peer4", &pk_b).unwrap();

        let cipher = a.encrypt("peer4", ctx, b"once").unwrap();
        assert_eq!(b.decrypt("peer4", ctx, &cipher).unwrap(), b"once");
        assert!(
            b.decrypt("peer4", ctx, &cipher).is_err(),
            "replay must fail"
        );

        let mut tampered = cipher;
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(
            b.decrypt("peer4", ctx, &tampered).is_err(),
            "tamper must fail"
        );
    }

    #[test]
    fn encrypt_without_handshake_fails() {
        let a = PqcLinkCrypto::new();
        let err = a.encrypt("peer5", "test:peer5", b"x").unwrap_err();
        assert!(err.contains("no ratchet session"));
    }

    #[test]
    fn encrypt_with_incomplete_handshake_fails() {
        let a = PqcLinkCrypto::new();
        a.begin_handshake("peer6", "test:peer6").unwrap();
        let err = a.encrypt("peer6", "test:peer6", b"x").unwrap_err();
        assert!(err.contains("handshake incomplete"));
    }

    #[test]
    fn oversized_payload_rejected() {
        let a = PqcLinkCrypto::new();
        let b = PqcLinkCrypto::new();
        let ctx = "test:peer7";
        let pk_a = a.begin_handshake("peer7", ctx).unwrap();
        let (_sid_b, pk_b) = b
            .accept_handshake(
                ctx,
                "peer7",
                &PqcLinkCrypto::handshake_frame("peer7", &pk_a),
            )
            .unwrap();
        a.complete_handshake("peer7", &pk_b).unwrap();
        let big = vec![0xABu8; MAX_LINK_PAYLOAD + 1];
        assert!(a.encrypt("peer7", ctx, &big).is_err());
    }

    #[test]
    fn bad_frames_rejected() {
        let a = PqcLinkCrypto::new();
        assert!(a.decrypt("peer8", "test:peer8", &[]).is_err());
        assert!(a.decrypt("peer8", "test:peer8", &[0x99]).is_err());
        assert!(a
            .decrypt("peer8", "test:peer8", &[FRAME_TAG_RATCHET, 0xFF, 0xFF])
            .is_err());
        assert!(a
            .accept_handshake("test:peer8", "peer8", &[0x99, 0x01])
            .is_err());
        assert!(a
            .accept_handshake("test:peer8", "peer8", &[FRAME_TAG_HANDSHAKE])
            .is_err());
        assert!(a
            .accept_handshake(
                "test:peer8",
                "peer8",
                &[FRAME_TAG_HANDSHAKE, 0, 0, 0, 99, b'x']
            )
            .is_err());
    }

    #[test]
    fn remove_drops_session() {
        let a = PqcLinkCrypto::new();
        a.begin_handshake("peer9", "test:peer9").unwrap();
        assert!(a.has_session("peer9"));
        a.remove("peer9");
        assert!(!a.has_session("peer9"));
    }

    #[test]
    fn duplicate_handshake_rejected() {
        let a = PqcLinkCrypto::new();
        a.begin_handshake("peer10", "test:peer10").unwrap();
        assert!(a.begin_handshake("peer10", "test:peer10").is_err());
        let b = PqcLinkCrypto::new();
        let frame = PqcLinkCrypto::handshake_frame("peer10", "abc");
        b.accept_handshake("test:peer10", "peer10", &frame).unwrap();
        assert!(b.accept_handshake("test:peer10", "peer10", &frame).is_err());
    }
}
