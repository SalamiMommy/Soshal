//! Cryptographic helpers for the PQC Double Ratchet v3.
//!
//! Compression (deflate), decompression (inflate, legacy base64 fallback),
//! and HKDF key derivation for the root chain, chain ratchets and message
//! keys.
//! All derivation is domain-separated with the v3 ratchet domain.

use base64::{engine::general_purpose, Engine as _};

use crate::hash::hkdf_sha256;
use crate::pqc_ratchet::hex_decode;

pub const RATCHET_DOMAIN: &[u8] = b"soshal-ratchet-v3";
pub const INIT_SALT: &[u8] = b"soshal-ratchet-v3-init";
pub const CHAIN_SALT: &[u8] = b"soshal-ratchet-v3-chain";
pub const ROOT_INFO: &[u8] = b"soshal-ratchet-v3:root";
pub const CHAIN_INFO: &[u8] = b"soshal-ratchet-v3:chain";
pub const MAX_INPUT_LEN: usize = 64 * 1024;

/// Raw deflate of the serialized JSON — no base64, no "z:" prefix.
pub fn compress_json(text: &str) -> Result<Vec<u8>, &'static str> {
    if text.len() > MAX_INPUT_LEN {
        return Err("input too large");
    }
    let serialized = serde_json::to_string(text).map_err(|_| "json ser err")?;
    soshal_content_core::compress::compress(serialized.as_bytes()).map_err(|_| "deflate err")
}

/// Inflate raw deflate bytes; a "z:" prefix routes to the legacy
/// base64-wrapped path written by older builds. Output is capped at 4 MiB to
/// prevent zip-bomb expansion (delegates the streaming cap to content-core).
/// Integrity is provided by the NIP-44 HMAC at the ratchet layer, not here.
pub fn decompress_json(compressed: &[u8]) -> Result<Vec<u8>, &'static str> {
    if compressed.starts_with(b"z:") {
        let src = std::str::from_utf8(&compressed[2..]).map_err(|_| "bad utf8")?;
        let bytes = general_purpose::STANDARD
            .decode(src)
            .map_err(|_| "bad b64")?;
        return soshal_content_core::compress::decompress_limited(&bytes, MAX_OUTPUT)
            .map_err(|_| "inflate err");
    }
    soshal_content_core::compress::decompress_limited(compressed, MAX_OUTPUT)
        .map_err(|_| "inflate err")
}

const MAX_OUTPUT: usize = 4 * 1024 * 1024;

fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8], len: usize) -> Result<Vec<u8>, &'static str> {
    hkdf_sha256(ikm, salt, info, len).map_err(|_| "hkdf failed")
}

/// Session-init root: `HKDF(ss, v3-init salt, context)` — derived by the
/// initiator from its encapsulate to the peer's static key and by the
/// responder from the decapsulation of the first received header.
pub fn init_root(shared_secret: &[u8], context: &str) -> Result<[u8; 32], &'static str> {
    let info = format!("soshal-ratchet-v3:{context}");
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
    let info = format!("soshal-ratchet-v3:root:{context}");
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
    let info = format!("soshal-ratchet-v3:chain:{context}");
    let out = hkdf(&root, CHAIN_SALT, info.as_bytes(), 32)?;
    let mut chain = [0u8; 32];
    chain.copy_from_slice(&out);
    Ok(chain)
}

pub fn derive_msg_key_bytes(
    chain_key: &[u8],
    context: &str,
) -> Result<([u8; 32], Vec<u8>), &'static str> {
    let mut info = Vec::with_capacity(16 + context.len());
    info.extend_from_slice(b"pqc-ratchet-msg-v3:");
    info.extend_from_slice(context.as_bytes());

    let out =
        soshal_pqc_core::hkdf::hkdf_sha256(chain_key, b"soshal-pqc-ratchet-msg-salt-v3", &info, 64)
            .map_err(|_| "hkdf failed")?;

    let mut msg_key = [0u8; 32];
    msg_key.copy_from_slice(&out[..32]);
    let next_chain = out[32..64].to_vec();
    Ok((msg_key, next_chain))
}

pub fn derive_msg_key(
    chain_key_hex: &str,
    context: &str,
) -> Result<([u8; 32], Vec<u8>), &'static str> {
    let chain_key = super::hex_decode(chain_key_hex)?;
    derive_msg_key_bytes(&chain_key, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pqc_ratchet::{decrypt_ratchet, encrypt_ratchet, init_state, RatchetInput};

    fn make_ratchet_pair(ctx: &str) -> (RatchetInput, RatchetInput) {
        let (bob_pk, bob_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
        let (alice_pk, alice_sk) = soshal_pqc_core::hybrid::hybrid_keygen().unwrap();
        let bob = init_state("", ctx, &bob_sk, &bob_pk);
        let alice = init_state(&bob_pk, ctx, &alice_sk, &alice_pk);
        (alice, bob)
    }

    #[test]
    fn test_ratchet_step_advances_state() {
        let ss = [0x11u8; 32];
        let root0 = init_root(&ss, "ctx").unwrap();
        let (root1, chain1) = derive_root_step(&ss, &hex::encode(root0), "ctx").unwrap();
        let (root2, chain2) = derive_root_step(&ss, &hex::encode(root1), "ctx").unwrap();
        assert_ne!(root0, root1);
        assert_ne!(root1, root2);
        assert_ne!(chain1, chain2);

        let (mk1, next1) = derive_msg_key(&hex::encode(chain1), "ctx").unwrap();
        let (mk2, next2) = derive_msg_key(&hex::encode(&next1), "ctx").unwrap();
        assert_ne!(mk1, mk2);
        assert_ne!(next1, next2);

        let (alice, _bob) = make_ratchet_pair("ctx");
        let (alice2, _h1, _c1) = encrypt_ratchet(&alice, "first").unwrap();
        let (alice3, _h2, _c2) = encrypt_ratchet(&alice2, "second").unwrap();
        assert_ne!(alice.sending_chain_key, alice2.sending_chain_key);
        assert_ne!(alice2.sending_chain_key, alice3.sending_chain_key);
        assert_eq!(alice3.sending_chain_counter, 2);
    }

    #[test]
    fn test_init_root_symmetric_both_directions() {
        let ss = b"shared-secret-from-kem";
        let alice_root = init_root(ss, "dm:alice:bob").unwrap();
        let bob_root = init_root(ss, "dm:alice:bob").unwrap();
        assert_eq!(alice_root, bob_root);
        let other_ctx = init_root(ss, "dm:bob:alice").unwrap();
        assert_ne!(alice_root, other_ctx);
        let other_ss = init_root(b"different-shared-secret", "dm:alice:bob").unwrap();
        assert_ne!(alice_root, other_ss);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_after_ratchet() {
        let (mut alice, mut bob) = make_ratchet_pair("dm:a:b");
        for i in 0..3 {
            let (a_out, h, ct) = encrypt_ratchet(&alice, &format!("msg-{i}")).unwrap();
            let (b_out, pt) = decrypt_ratchet(&bob, &h, &ct).unwrap();
            assert_eq!(pt, format!("msg-{i}"));
            alice = a_out;
            bob = b_out;
        }
        let (b_out, h, ct) = encrypt_ratchet(&bob, "reply from bob").unwrap();
        let (a_out, pt) = decrypt_ratchet(&alice, &h, &ct).unwrap();
        assert_eq!(pt, "reply from bob");
        assert_eq!(a_out.chain_counter, b_out.chain_counter);
    }

    #[test]
    fn test_wrong_ratchet_state_fails_decryption() {
        let (alice, bob) = make_ratchet_pair("dm:a:b");
        let (alice2, h, ct) = encrypt_ratchet(&alice, "secret").unwrap();
        let (bob2, pt) = decrypt_ratchet(&bob, &h, &ct).unwrap();
        assert_eq!(pt, "secret");

        let (eve, _) = make_ratchet_pair("dm:a:b");
        assert!(decrypt_ratchet(&eve, &h, &ct).is_err());

        let mut tampered = ct.clone();
        tampered.push('!');
        assert!(decrypt_ratchet(&bob, &h, &tampered).is_err());

        assert!(decrypt_ratchet(&bob2, &h, &ct).is_err());

        let (_, h2, ct2) = encrypt_ratchet(&alice2, "next").unwrap();
        let far_ahead = crate::pqc_ratchet::HeaderOutput {
            version: crate::pqc_ratchet::RATCHET_VERSION,
            pk: h2.pk,
            ct: h2.ct,
            seq: 0,
            chain_counter: bob2.chain_counter + 2,
            header_mac: String::new(),
        };
        assert!(decrypt_ratchet(&bob2, &far_ahead, &ct2).is_err());
    }

    #[test]
    fn test_compress_decompress_roundtrip_and_tamper() {
        let original = "hello ratchet, compressed with deflate";
        let compressed = compress_json(original).unwrap();
        assert_eq!(
            decompress_json(&compressed).unwrap(),
            serde_json::to_vec(original).unwrap()
        );

        // Raw deflate has no integrity layer — tamper detection lives in the
        // NIP-44 HMAC at the ratchet level (covered by
        // test_wrong_ratchet_state_fails_decryption). Here assert the
        // decompressor rejects non-deflate input.
        assert!(decompress_json(b"\xff\xff\xff\xff\xff").is_err());

        let legacy = format!(
            "z:{}",
            general_purpose::STANDARD.encode(
                soshal_content_core::compress::compress(&serde_json::to_vec(original).unwrap())
                    .unwrap()
            )
        );
        assert_eq!(
            decompress_json(legacy.as_bytes()).unwrap(),
            serde_json::to_vec(original).unwrap()
        );

        assert!(decompress_json(b"z:!!!not-base64").is_err());
        assert!(decompress_json(b"plain text without z prefix").is_err());

        let big = "x".repeat(MAX_INPUT_LEN + 1);
        assert!(compress_json(&big).is_err());
    }
}
