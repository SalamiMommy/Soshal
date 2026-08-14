//! Cryptographic helpers for the PQC Double Ratchet v3.
//!
//! Compression (deflate + base64), decompression (base64 + inflate), and
//! HKDF key derivation for the root chain, chain ratchets and message keys.
//! All derivation is domain-separated with the v3 ratchet domain.

use base64::{engine::general_purpose, Engine as _};

use crate::hash::hkdf_sha256;
use crate::pqc_ratchet::hex_decode;

pub const RATCHET_DOMAIN: &[u8] = b"soshal-ratchet-v3";
pub const INIT_SALT: &[u8] = b"soshal-ratchet-v3-init";
pub const CHAIN_SALT: &[u8] = b"soshal-ratchet-v3-chain";
pub const ROOT_INFO: &[u8] = b"soshal-ratchet-v3:root";
pub const CHAIN_INFO: &[u8] = b"soshal-ratchet-v3:chain";
pub const MSG_INFO: &[u8] = b"soshal-ratchet-v3:msg";
pub const MAX_INPUT_LEN: usize = 64 * 1024;

/// Deflate + base64 + "z:" prefix (compatible with TS compressJson).
pub fn compress_json(text: &str) -> Result<String, &'static str> {
    if text.len() > MAX_INPUT_LEN {
        return Err("input too large");
    }
    let serialized = serde_json::to_string(text).map_err(|_| "json ser err")?;
    let compressed = soshal_content_core::compress::compress(serialized.as_bytes())
        .map_err(|_| "deflate err")?;
    let b64 = general_purpose::STANDARD.encode(&compressed);
    Ok(format!("z:{}", b64))
}

/// Strip "z:" prefix, base64 decode, inflate. Output is capped at 4 MiB to
/// prevent zip-bomb expansion (delegates the streaming cap to content-core).
pub fn decompress_json(compressed: &str) -> Result<Vec<u8>, &'static str> {
    let src = compressed.strip_prefix("z:").unwrap_or(compressed);
    let bytes = general_purpose::STANDARD
        .decode(src)
        .map_err(|_| "bad b64")?;
    soshal_content_core::compress::decompress_limited(&bytes, MAX_OUTPUT).map_err(|_| "inflate err")
}

const MAX_OUTPUT: usize = 4 * 1024 * 1024;

fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8], len: usize) -> Result<Vec<u8>, &'static str> {
    hkdf_sha256(ikm, salt, info, len).map_err(|_| "hkdf failed")
}

/// Session-init root: `HKDF(ss, v3-init salt, context)` — derived by the
/// initiator from its encapsulate to the peer's static key and by the
/// responder from the decapsulation of the first received header.
pub fn init_root(shared_secret: &[u8], context: &str) -> Result<[u8; 32], &'static str> {
    let info = format!("{}:{}", String::from_utf8_lossy(RATCHET_DOMAIN), context);
    let root = hkdf(shared_secret, INIT_SALT, info.as_bytes(), 32)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&root);
    Ok(out)
}

/// Root-chain step: `HKDF(ss, root, ROOT_INFO, 64)` → new root ‖ new chain
/// key. Applied once per epoch by both sides.
pub fn derive_root_step(
    shared_secret: &[u8],
    root_key_hex: &str,
    context: &str,
) -> Result<([u8; 32], [u8; 32]), &'static str> {
    let root = hex_decode(root_key_hex).map_err(|_| "bad root hex")?;
    let info = format!("{}:{}", String::from_utf8_lossy(ROOT_INFO), context);
    let out = hkdf(shared_secret, &root, info.as_bytes(), 64)?;
    let mut new_root = [0u8; 32];
    let mut new_chain = [0u8; 32];
    new_root.copy_from_slice(&out[..32]);
    new_chain.copy_from_slice(&out[32..64]);
    Ok((new_root, new_chain))
}

/// Derives a fresh sending/receiving chain from the root key.
pub fn derive_chain(root_key_hex: &str, context: &str) -> Result<[u8; 32], &'static str> {
    let root = hex_decode(root_key_hex).map_err(|_| "bad root hex")?;
    let info = format!("{}:{}", String::from_utf8_lossy(CHAIN_INFO), context);
    let out = hkdf(&root, CHAIN_SALT, info.as_bytes(), 32)?;
    let mut chain = [0u8; 32];
    chain.copy_from_slice(&out);
    Ok(chain)
}

pub fn derive_msg_key(
    chain_key_hex: &str,
    context: &str,
) -> Result<([u8; 32], Vec<u8>), &'static str> {
    let chain_key = super::hex_decode(chain_key_hex)?;

    let mut info = Vec::with_capacity(16 + context.len());
    info.extend_from_slice(b"pqc-ratchet-msg-v3:");
    info.extend_from_slice(context.as_bytes());

    let out = soshal_pqc_core::hkdf::hkdf_sha256(
        &chain_key,
        b"soshal-pqc-ratchet-msg-salt-v3",
        &info,
        64,
    )
    .map_err(|_| "hkdf failed")?;

    let mut msg_key = [0u8; 32];
    msg_key.copy_from_slice(&out[..32]);
    let next_chain = out[32..64].to_vec();
    Ok((msg_key, next_chain))
}
