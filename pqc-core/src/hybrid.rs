//! Hybrid KEM — X25519 (classic) + ML-KEM-768 (post-quantum) combined.
//!
//! Shared secret via NIST dual-PRF:
//! `ss = HKDF-SHA256(salt = "soshal-hybrid-v1", ikm = x25519_ss ‖ mlkem_ss, info = domain)`.
//! Security holds as long as at least one of the two KEMs is secure.
//!
//! Wire format (versioned, hard cut — no legacy pure-ML-KEM transport):
//! - public key: `0x01 ‖ x25519_pk(32) ‖ mlkem_pk(1184)` = 1217 bytes
//! - ciphertext: `0x01 ‖ x25519_ct(32) ‖ mlkem_ct(1088)` = 1121 bytes
//! - secret key: `x25519_sk(32) ‖ mlkem_seed(64)` = 96 bytes
//!
//! Any version byte != 1 is rejected.

use ml_kem::kem::Decapsulate;
use ml_kem::{DecapsulationKey768, EncapsulationKey768, MlKem768, Seed};
use ml_kem::{Encapsulate, KeyExport, TryKeyInit};
use x25519_dalek::{PublicKey as XPublicKey, SharedSecret, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

pub const HYBRID_VERSION: u8 = 1;
pub const X25519_PK_LEN: usize = 32;
pub const X25519_CT_LEN: usize = 32;
pub const MLKEM_PK_LEN: usize = 1184;
pub const MLKEM_CT_LEN: usize = 1088;
pub const HYBRID_PK_LEN: usize = 1 + X25519_PK_LEN + MLKEM_PK_LEN;
pub const HYBRID_CT_LEN: usize = 1 + X25519_CT_LEN + MLKEM_CT_LEN;
pub const HYBRID_SK_LEN: usize = X25519_PK_LEN + 64;
pub const SS_LEN: usize = 32;

pub const DUAL_PRF_SALT: &[u8] = b"soshal-hybrid-v1";

pub fn hybrid_keygen_bytes() -> Result<([u8; HYBRID_PK_LEN], [u8; HYBRID_SK_LEN]), String> {
    let mut x25519_sk_bytes = [0u8; X25519_PK_LEN];
    getrandom::fill(&mut x25519_sk_bytes).map_err(|e| format!("rng: {}", e))?;
    let x25519_sk = StaticSecret::from(x25519_sk_bytes);
    let x25519_pk = XPublicKey::from(&x25519_sk);

    let mut mlkem_seed_bytes = [0u8; 64];
    getrandom::fill(&mut mlkem_seed_bytes).map_err(|e| format!("rng: {}", e))?;
    let seed_arr = Zeroizing::new(
        Seed::try_from(mlkem_seed_bytes.as_slice()).map_err(|e| format!("seed: {}", e))?,
    );
    let dk = DecapsulationKey768::from(*seed_arr);
    let ek = dk.encapsulation_key();
    let ek_bytes = ek.to_bytes();

    let mut pk = [0u8; HYBRID_PK_LEN];
    pk[0] = HYBRID_VERSION;
    pk[1..1 + X25519_PK_LEN].copy_from_slice(x25519_pk.as_bytes());
    pk[1 + X25519_PK_LEN..].copy_from_slice(ek_bytes.as_slice());

    let mut sk = [0u8; HYBRID_SK_LEN];
    sk[..X25519_PK_LEN].copy_from_slice(&x25519_sk_bytes);
    sk[X25519_PK_LEN..].copy_from_slice(&mlkem_seed_bytes);
    x25519_sk_bytes.zeroize();
    mlkem_seed_bytes.zeroize();
    Ok((pk, sk))
}

pub fn hybrid_encapsulate_bytes(
    pk_bytes: &[u8; HYBRID_PK_LEN],
    domain: &[u8],
) -> Result<([u8; HYBRID_CT_LEN], [u8; SS_LEN]), String> {
    if pk_bytes[0] != HYBRID_VERSION {
        return Err("unsupported hybrid pk version".to_string());
    }
    let mut x25519_peer_arr = [0u8; X25519_PK_LEN];
    x25519_peer_arr.copy_from_slice(&pk_bytes[1..1 + X25519_PK_LEN]);
    let peer_xpk = XPublicKey::from(x25519_peer_arr);

    let mut eph_sk_bytes = [0u8; X25519_PK_LEN];
    getrandom::fill(&mut eph_sk_bytes).map_err(|e| format!("rng: {}", e))?;
    let eph_sk = StaticSecret::from(eph_sk_bytes);
    let eph_pk = XPublicKey::from(&eph_sk);
    let mut x25519_ss: SharedSecret = eph_sk.diffie_hellman(&peer_xpk);

    let ek = EncapsulationKey768::new_from_slice(&pk_bytes[1 + X25519_PK_LEN..])
        .map_err(|_| "bad ek".to_string())?;
    let (mlkem_ct, mut mlkem_ss) =
        ek.encapsulate_with_rng(&mut rand_core::UnwrapErr(getrandom::SysRng));

    let mut ct = [0u8; HYBRID_CT_LEN];
    ct[0] = HYBRID_VERSION;
    ct[1..1 + X25519_CT_LEN].copy_from_slice(eph_pk.as_bytes());
    ct[1 + X25519_CT_LEN..].copy_from_slice(mlkem_ct.as_slice());

    let mut ikm = [0u8; SS_LEN + SS_LEN];
    ikm[..SS_LEN].copy_from_slice(x25519_ss.as_bytes());
    ikm[SS_LEN..].copy_from_slice(mlkem_ss.as_slice());
    let mut ss = [0u8; SS_LEN];
    let derived = crate::hkdf::hkdf_sha256(&ikm, DUAL_PRF_SALT, domain, SS_LEN)?;
    ss.copy_from_slice(&derived);
    ikm.zeroize();

    x25519_ss.zeroize();
    mlkem_ss.zeroize();
    eph_sk_bytes.zeroize();
    Ok((ct, ss))
}

pub fn hybrid_decapsulate_bytes(
    ct_bytes: &[u8; HYBRID_CT_LEN],
    sk_bytes: &[u8; HYBRID_SK_LEN],
    domain: &[u8],
) -> Result<[u8; SS_LEN], String> {
    if ct_bytes[0] != HYBRID_VERSION {
        return Err("unsupported hybrid ct version".to_string());
    }
    let mut x25519_sk_arr = [0u8; X25519_PK_LEN];
    x25519_sk_arr.copy_from_slice(&sk_bytes[..X25519_PK_LEN]);
    let x25519_sk = StaticSecret::from(x25519_sk_arr);

    let mut eph_pk_arr = [0u8; X25519_CT_LEN];
    eph_pk_arr.copy_from_slice(&ct_bytes[1..1 + X25519_CT_LEN]);
    let eph_pk = XPublicKey::from(eph_pk_arr);
    let mut x25519_ss: SharedSecret = x25519_sk.diffie_hellman(&eph_pk);

    let seed_arr = Zeroizing::new(
        Seed::try_from(&sk_bytes[X25519_PK_LEN..]).map_err(|_| "bad seed".to_string())?,
    );
    let dk = DecapsulationKey768::from(*seed_arr);
    let mlkem_ct = ml_kem::Ciphertext::<MlKem768>::try_from(&ct_bytes[1 + X25519_CT_LEN..])
        .map_err(|_| "bad ct".to_string())?;
    let mut mlkem_ss = dk.decapsulate(&mlkem_ct);

    let mut ikm = [0u8; SS_LEN + SS_LEN];
    ikm[..SS_LEN].copy_from_slice(x25519_ss.as_bytes());
    ikm[SS_LEN..].copy_from_slice(mlkem_ss.as_slice());
    let mut ss = [0u8; SS_LEN];
    let derived = crate::hkdf::hkdf_sha256(&ikm, DUAL_PRF_SALT, domain, SS_LEN)?;
    ss.copy_from_slice(&derived);
    ikm.zeroize();

    x25519_ss.zeroize();
    mlkem_ss.zeroize();
    Ok(ss)
}

/// Generates a new hybrid keypair, returning `(pk_hex, sk_hex)`.
pub fn hybrid_keygen() -> Result<(String, String), String> {
    let (pk, sk) = hybrid_keygen_bytes()?;
    Ok((hex::encode(pk), hex::encode(sk)))
}

pub fn hybrid_encapsulate(pk_hex: &str, domain: &[u8]) -> Result<(String, String), String> {
    let pk_bytes = hex::decode(pk_hex).map_err(|_| "bad pk hex".to_string())?;
    if pk_bytes.len() != HYBRID_PK_LEN {
        return Err("bad pk len".to_string());
    }
    let mut pk_arr = [0u8; HYBRID_PK_LEN];
    pk_arr.copy_from_slice(&pk_bytes);
    let (ct, ss) = hybrid_encapsulate_bytes(&pk_arr, domain)?;
    Ok((hex::encode(ct), hex::encode(ss)))
}

pub fn hybrid_decapsulate(ct_hex: &str, sk_hex: &str, domain: &[u8]) -> Result<String, String> {
    let ct_bytes = hex::decode(ct_hex).map_err(|_| "bad ct hex".to_string())?;
    let sk_bytes = Zeroizing::new(hex::decode(sk_hex).map_err(|_| "bad sk hex".to_string())?);
    if ct_bytes.len() != HYBRID_CT_LEN || sk_bytes.len() != HYBRID_SK_LEN {
        return Err("bad input length".to_string());
    }
    let mut ct_arr = [0u8; HYBRID_CT_LEN];
    let mut sk_arr = [0u8; HYBRID_SK_LEN];
    ct_arr.copy_from_slice(&ct_bytes);
    sk_arr.copy_from_slice(&sk_bytes);
    let res = hybrid_decapsulate_bytes(&ct_arr, &sk_arr, domain);
    sk_arr.zeroize();
    Ok(hex::encode(res?))
}
