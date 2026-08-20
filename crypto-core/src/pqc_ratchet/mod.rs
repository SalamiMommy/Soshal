//! PQC Double Ratchet v3 — hybrid KEM (X25519 + ML-KEM-768) ratchet.
//!
//! True double-ratchet structure:
//! - **Root chain**: advances on every chain-epoch transition via a hybrid
//!   KEM ratchet step — the sender encapsulates to the peer's current
//!   published hybrid public key, the receiver decapsulates with its own
//!   current secret key, then rotates its keypair and republishes the
//!   public key (existing publish/fetch pattern retained).
//! - **Sending / receiving symmetric chains**: separate HKDF chain ratchets.
//!   Message keys come from the active chain's sequence position — never from
//!   root+seq — so multiple messages in one epoch share one root step and the
//!   receiving chain decrypts them in order, out of order, or with gaps via
//!   the skipped-key buffer (≤ 1024 keys).
//! - **Header v3**: `version ‖ pk ‖ ct ‖ seq ‖ chain_counter`. The version
//!   byte is mandatory: v2 and older headers are rejected hard — there is no
//!   legacy decoding path.
//! - **Init**: the first message of a session carries the hybrid encapsulate
//!   to the peer's published static hybrid key; both sides derive the session
//!   root as `HKDF(ss, "soshal-ratchet-v3-init", context)`.

use zeroize::Zeroize;

use self::ratchet_crypto::{
    compress_json, decompress_json, derive_chain, derive_msg_key, derive_msg_key_bytes,
    derive_root_step, init_root, RATCHET_DOMAIN,
};
use crate::nip44::{decrypt as nip44_decrypt, encrypt as nip44_encrypt};
use soshal_pqc_core::hybrid::{
    hybrid_decapsulate, hybrid_encapsulate, hybrid_keygen, HYBRID_CT_LEN, HYBRID_PK_LEN,
};

pub mod ratchet_crypto;

// ─── Constants ────────────────────────────────────────────────────────

/// Ratchet wire version. Headers with any other version are rejected.
pub const RATCHET_VERSION: u8 = 3;

/// Maximum skipped message keys kept for out-of-order delivery, and the
/// maximum sequence jump a decrypt will attempt within an epoch. Bounds both
/// memory and decapsulation work (availability limiter).
pub const MAX_RATCHET_WINDOW: i64 = 1024;

/// Upper bound for a ratchet ciphertext payload (raw-deflate NIP-44 output).
/// Real messages are a few KiB; the cap keeps forged oversized
/// payloads from forcing large allocations inside the decrypt path.
pub const MAX_RATCHET_CIPHERTEXT: usize = 64 * 1024;

pub fn hex_decode(hex_str: &str) -> Result<Vec<u8>, &'static str> {
    if !hex_str.len().is_multiple_of(2) {
        return Err("odd hex length");
    }
    if hex_str.len() > 512 * 1024 {
        return Err("hex too large");
    }
    hex::decode(hex_str).map_err(|_| "invalid hex")
}

// ─── State types ──────────────────────────────────────────────────────

/// A skipped message key buffered for out-of-order delivery within the
/// current receiving epoch.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct SkippedKey {
    pub seq: i64,
    pub key: String,
}

/// Ratchet session state. Serialized (at-rest encrypted) per peer; the
/// `version` field makes stale v2 blobs fail parsing so they are discarded
/// and a fresh session is initialized.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct RatchetState {
    /// Wire version — always 3; any other value fails parse/serde.
    pub version: u8,
    /// Root chain key (hex, 32 bytes); empty until the first KEM step.
    pub root_key: String,
    /// Own current hybrid secret key (hex, 96 bytes).
    pub current_sk: String,
    /// Own current hybrid public key (hex, 1217 bytes) — sent in headers and
    /// published after rotation so the peer can encapsulate to it.
    pub current_pk: String,
    /// Peer's current hybrid public key (hex). Set from fetched publications
    /// on send and from the last received header on receive.
    pub peer_pk: String,
    /// Per-pair session context (e.g. `dm:<pubkey>:<pubkey>`).
    pub context: String,
    /// Root-step count. Every epoch transition increments it on both sides.
    pub chain_counter: i64,
    /// Sending chain key (hex, 32 bytes); empty until the first send.
    pub sending_chain_key: String,
    /// Next sequence number to use in the sending chain.
    pub sending_chain_counter: i64,
    /// Peer pk the sending chain was built against; a change forces a new
    /// epoch (root step) on the next send.
    pub sending_epoch_peer_pk: String,
    /// Ciphertext of the current sending epoch's KEM step (hex, 1121 bytes).
    /// Repeated in every header of the epoch so a receiver that missed the
    /// first message can still perform the root step.
    pub sending_ct: String,
    /// Receiving chain key (hex, 32 bytes); empty until the first receive.
    pub receiving_chain_key: String,
    /// Next expected sequence in the receiving chain.
    pub receiving_chain_counter: i64,
    /// Skipped-key buffer for out-of-order delivery (≤ MAX_RATCHET_WINDOW).
    pub skipped: Vec<SkippedKey>,
}

impl Drop for RatchetState {
    fn drop(&mut self) {
        self.root_key.zeroize();
        self.current_sk.zeroize();
        self.sending_chain_key.zeroize();
        self.receiving_chain_key.zeroize();
        for s in &mut self.skipped {
            s.key.zeroize();
        }
    }
}

/// Input state for ratchet operations (deserialized from at-rest storage).
pub type RatchetInput = RatchetState;

/// Output state after a ratchet operation (persisted to at-rest storage).
pub type RatchetOutput = RatchetState;

/// Per-message ratchet header (v3).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct HeaderOutput {
    pub version: u8,
    pub pk: String,
    pub ct: String,
    pub seq: i64,
    pub chain_counter: i64,
}

#[derive(serde::Serialize)]
pub struct EncryptOutput {
    #[serde(flatten)]
    pub state: RatchetOutput,
    pub header: HeaderOutput,
    pub ciphertext: String,
}

#[derive(serde::Serialize)]
pub struct DecryptOutput {
    #[serde(flatten)]
    pub state: RatchetOutput,
    pub plaintext: String,
}

// ─── Session init ─────────────────────────────────────────────────────

/// Creates an empty v3 ratchet state bound to the peer's published hybrid
/// static key. The root key is empty until the first KEM root step: the
/// initiator derives it from its encapsulate to `peer_pk` on first send, the
/// responder from the decapsulation of the first received header.
pub fn init_state(
    peer_pk: &str,
    context: &str,
    current_sk: &str,
    current_pk: &str,
) -> RatchetOutput {
    RatchetOutput {
        version: RATCHET_VERSION,
        root_key: String::new(),
        current_sk: current_sk.to_string(),
        current_pk: current_pk.to_string(),
        peer_pk: peer_pk.to_string(),
        context: context.to_string(),
        chain_counter: 0,
        sending_chain_key: String::new(),
        sending_chain_counter: 0,
        sending_epoch_peer_pk: String::new(),
        sending_ct: String::new(),
        receiving_chain_key: String::new(),
        receiving_chain_counter: 0,
        skipped: Vec::new(),
    }
}

// ─── Encrypt ──────────────────────────────────────────────────────────

pub fn encrypt_ratchet(
    state: &RatchetInput,
    plaintext: &str,
) -> Result<(RatchetOutput, HeaderOutput, String), &'static str> {
    if state.version != RATCHET_VERSION {
        return Err("ratchet v3 required");
    }
    if state.peer_pk.is_empty() {
        return Err("no peer public key");
    }
    let need_new_epoch =
        state.sending_chain_key.is_empty() || state.peer_pk != state.sending_epoch_peer_pk;
    let (
        root_key,
        mut sending_chain_key,
        chain_counter,
        seq,
        sending_epoch_peer_pk,
        sending_ct,
        current_sk,
        current_pk,
    ) = if need_new_epoch {
        let (ct_hex, ss_hex) =
            hybrid_encapsulate(&state.peer_pk, RATCHET_DOMAIN).map_err(|_| "bad peer pk hex")?;
        let mut ss = hex_decode(&ss_hex).map_err(|_| "bad ss hex")?;
        let (root_hex, chain_hex) = if state.root_key.is_empty() {
            // Session init: root = HKDF(ss, v3-init salt, context).
            let root = init_root(&ss, &state.context)?;
            ss.zeroize();
            let root_hex = hex::encode(root);
            let chain = derive_chain(&root_hex, &state.context)?;
            (root_hex, hex::encode(chain))
        } else {
            let (new_root, new_chain) = derive_root_step(&ss, &state.root_key, &state.context)?;
            ss.zeroize();
            (hex::encode(new_root), hex::encode(new_chain))
        };
        // Rotate own keypair: the fresh pk travels in the header and the
        // sk decrypts the peer's replies until the next root step.
        let (pk, sk) = hybrid_keygen().map_err(|_| "rng failed")?;
        (
            root_hex,
            chain_hex,
            state.chain_counter + 1,
            0i64,
            state.peer_pk.clone(),
            ct_hex,
            sk,
            pk,
        )
    } else {
        (
            state.root_key.clone(),
            state.sending_chain_key.clone(),
            state.chain_counter,
            state.sending_chain_counter,
            state.sending_epoch_peer_pk.clone(),
            state.sending_ct.clone(),
            state.current_sk.clone(),
            state.current_pk.clone(),
        )
    };

    let (msg_key, next_chain) = derive_msg_key(&sending_chain_key, &state.context)?;
    sending_chain_key = hex::encode(&next_chain);

    let compressed = compress_json(plaintext)?;
    let mut mk_arr = [0u8; 32];
    mk_arr.copy_from_slice(&msg_key);
    let ciphertext = nip44_encrypt(&compressed, &mk_arr)?;
    let mut msg_key_hex = hex::encode(msg_key);
    msg_key_hex.zeroize();

    let header = HeaderOutput {
        version: RATCHET_VERSION,
        pk: current_pk.clone(),
        ct: sending_ct.clone(),
        seq,
        chain_counter,
    };

    let out = RatchetOutput {
        version: RATCHET_VERSION,
        root_key,
        current_sk,
        current_pk,
        peer_pk: state.peer_pk.clone(),
        context: state.context.clone(),
        chain_counter,
        sending_chain_key,
        sending_chain_counter: seq + 1,
        sending_epoch_peer_pk,
        sending_ct,
        receiving_chain_key: state.receiving_chain_key.clone(),
        receiving_chain_counter: state.receiving_chain_counter,
        skipped: state.skipped.clone(),
    };
    Ok((out, header, ciphertext))
}

// ─── Decrypt ──────────────────────────────────────────────────────────

pub fn decrypt_ratchet(
    state: &RatchetInput,
    header: &HeaderOutput,
    ciphertext: &str,
) -> Result<(RatchetOutput, String), &'static str> {
    if state.version != RATCHET_VERSION {
        return Err("ratchet v3 required");
    }
    // Hard cut: no legacy header versions are decoded.
    if header.version != RATCHET_VERSION {
        return Err("unsupported ratchet header version");
    }
    // Cheap pre-checks before any KEM work: a forged header with oversized
    // hex fields must be rejected with string-length checks only, so a
    // spammer cannot force large allocations or KEM decapsulation work.
    if header.pk.len() != HYBRID_PK_LEN * 2 {
        return Err("bad pk size");
    }
    if header.ct.len() != HYBRID_CT_LEN * 2 {
        return Err("bad ct size");
    }
    if ciphertext.len() > MAX_RATCHET_CIPHERTEXT {
        return Err("ciphertext too large");
    }
    if header.chain_counter < state.chain_counter {
        return Err("replay");
    }
    if header.chain_counter > state.chain_counter + 1 {
        return Err("message too far ahead of ratchet (lost messages; re-sync required)");
    }
    let mut out = state.clone();
    let cc_transition = header.chain_counter == state.chain_counter + 1;

    let mut recv_chain: Vec<u8>;
    let prev_counter: i64;
    if cc_transition {
        let ss_hex = hybrid_decapsulate(&header.ct, &state.current_sk, RATCHET_DOMAIN)
            .map_err(|_| "bad ct/sk hex")?;
        let mut ss = hex_decode(&ss_hex).map_err(|_| "bad ss hex")?;
        if state.root_key.is_empty() {
            let root = init_root(&ss, &state.context)?;
            ss.zeroize();
            out.root_key.zeroize();
            out.root_key = hex::encode(root);
            recv_chain = derive_chain(&out.root_key, &state.context)?.to_vec();
        } else {
            let (new_root, new_chain) = derive_root_step(&ss, &state.root_key, &state.context)?;
            ss.zeroize();
            out.root_key.zeroize();
            out.root_key = hex::encode(new_root);
            recv_chain = new_chain.to_vec();
        }
        out.chain_counter = header.chain_counter;
        out.receiving_chain_counter = 0;
        for s in &mut out.skipped {
            s.key.zeroize();
        }
        out.skipped.clear();
        // Rotate the receiver KEM keypair after every root step and
        // republish the new public key (forward secrecy): a compromised
        // receiver key cannot decrypt future messages.
        let (rotated_pk, rotated_sk) = hybrid_keygen().map_err(|_| "rng failed")?;
        out.current_sk.zeroize();
        out.current_sk = rotated_sk;
        out.current_pk = rotated_pk;
        prev_counter = 0;
    } else {
        if state.receiving_chain_key.is_empty() {
            return Err("no receiving chain for chain_counter");
        }
        let chain_bytes = hex_decode(&state.receiving_chain_key).map_err(|_| "bad chain hex")?;
        recv_chain = chain_bytes;
        prev_counter = state.receiving_chain_counter;
    }

    // Message-key extraction: chain advance with skipped-key buffer.
    let msg_key: [u8; 32];
    if header.seq < prev_counter {
        let pos = out
            .skipped
            .iter()
            .position(|s| s.seq == header.seq)
            .ok_or("replay")?;
        let mut key_hex = out.skipped.remove(pos).key;
        let mut bytes = match hex_decode(&key_hex) {
            Ok(b) => b,
            Err(e) => {
                key_hex.zeroize();
                return Err(e);
            }
        };
        key_hex.zeroize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        bytes.zeroize();
        msg_key = arr;
    } else {
        let gap = header.seq - prev_counter;
        if gap > MAX_RATCHET_WINDOW {
            return Err("message too far ahead of ratchet (lost messages; re-sync required)");
        }
        for i in 0..gap {
            let (skipped_key, next) = derive_msg_key_bytes(&recv_chain, &state.context)?;
            recv_chain = next;
            out.skipped.push(SkippedKey {
                seq: prev_counter + i,
                key: hex::encode(skipped_key),
            });
            if out.skipped.len() > MAX_RATCHET_WINDOW as usize {
                let mut evicted = out.skipped.remove(0);
                evicted.key.zeroize();
            }
        }
        let (mk, next) = derive_msg_key_bytes(&recv_chain, &state.context)?;
        recv_chain = next;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&mk);
        msg_key = arr;
        out.receiving_chain_key.zeroize();
        out.receiving_chain_key = hex::encode(&recv_chain);
        out.receiving_chain_counter = header.seq + 1;
    }

    let compressed_bytes = nip44_decrypt(ciphertext, &msg_key)?;
    let inflated = decompress_json(&compressed_bytes)?;
    let plaintext_value: serde_json::Value =
        serde_json::from_slice(&inflated).map_err(|_| "bad json")?;
    let final_plaintext = match plaintext_value {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    };

    // The peer's header pk is the live target for our next encapsulate.
    out.peer_pk = header.pk.clone();
    Ok((out, final_plaintext))
}

// ─── DM protocol helpers (state persistence, header parse, tags) ────────

/// Settings key holding the sealed ratchet state for a peer.
pub fn state_key(peer: &str) -> String {
    format!("pqc_state:{}", peer)
}

/// Domain-separation context for a peer pair. The two identity components
/// are sorted so both sides derive the identical string regardless of call
/// order ("dm:A:B" == "dm:B:A") — every key derivation bakes this string in,
/// so an asymmetric context would make the first frame fail to decrypt.
pub fn ratchet_context(my_pubkey: &str, peer: &str) -> String {
    let (first, second) = if my_pubkey <= peer {
        (my_pubkey, peer)
    } else {
        (peer, my_pubkey)
    };
    format!("dm:{}:{}", first, second)
}

/// Parses and version-checks ratchet state from decrypted plaintext. Stale
/// v2-era blobs fail parsing (missing v3 fields) and are discarded — the
/// caller initializes a fresh session; old messages are undecryptable by
/// design.
pub fn ratchet_state_from_plaintext(plaintext: &[u8]) -> Option<RatchetInput> {
    let state: RatchetInput = serde_json::from_str(std::str::from_utf8(plaintext).ok()?).ok()?;
    if state.version != RATCHET_VERSION {
        return None;
    }
    Some(state)
}

/// Serializes ratchet output state for storage.
pub fn ratchet_state_to_json(state: &RatchetOutput) -> Option<String> {
    serde_json::to_string(state).ok()
}

/// Assembles the wrapper tags for a published ratchet DM: the live ratchet pk
/// (not the static seed) so peers always encapsulate to our current
/// decapsulation key, plus the full header.
pub fn ratchet_wrapper_tags(out: &RatchetOutput, header: &HeaderOutput) -> Vec<Vec<String>> {
    vec![
        vec!["p".to_string(), out.peer_pk.clone()],
        vec!["pqc_pk".to_string(), out.current_pk.clone()],
        vec!["pqc_pk_version".to_string(), "1".to_string()],
        vec!["ratchet_pk".to_string(), header.pk.clone()],
        vec!["ratchet_ct".to_string(), header.ct.clone()],
        vec!["ratchet_seq".to_string(), header.seq.to_string()],
        vec!["ratchet_cc".to_string(), header.chain_counter.to_string()],
        vec!["ratchet_version".to_string(), header.version.to_string()],
    ]
}

/// Parses a v3 ratchet header from wrapper tags. Events without the
/// version/chain-counter tags (v2 era) yield `None` — hard cut, never decoded.
pub fn ratchet_header_from_tags(tags: &[Vec<String>]) -> Option<HeaderOutput> {
    let tag_of = |name: &str| {
        tags.iter()
            .find(|t| t.first().map(|s| s.as_str()) == Some(name))
            .and_then(|t| t.get(1).map(|s| s.to_string()))
    };
    let (h_pk, h_ct, h_seq, h_cc, h_version) = (
        tag_of("ratchet_pk")?,
        tag_of("ratchet_ct")?,
        tag_of("ratchet_seq")?,
        tag_of("ratchet_cc")?,
        tag_of("ratchet_version")?,
    );
    let (Ok(seq), Ok(cc), Ok(version)) = (
        h_seq.parse::<i64>(),
        h_cc.parse::<i64>(),
        h_version.parse::<u8>(),
    ) else {
        return None;
    };
    if version != RATCHET_VERSION {
        return None;
    }
    Some(HeaderOutput {
        version,
        pk: h_pk,
        ct: h_ct,
        seq,
        chain_counter: cc,
    })
}
