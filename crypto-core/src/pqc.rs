//! Post-quantum KEM (ML-KEM-768) and DSA (ML-DSA-65) primitives.
//!
//! Thin byte-array API over [`soshal_pqc_core`]; all cryptographic state
//! lives in pqc-core.

use soshal_pqc_core::dsa as pqc_dsa;
use soshal_pqc_core::kem as pqc_kem;

use crate::pqc_ratchet::hex_decode;

pub mod hybrid {
    //! Hybrid (X25519 + ML-KEM-768) KEM thin wrapper over soshal_pqc_core.

    pub const VERSION: u8 = 1;
    pub const PUBLIC_KEY_LEN: usize = 1217;
    pub const CIPHERTEXT_LEN: usize = 1121;
    pub const SECRET_KEY_LEN: usize = 96;
    pub const SHARED_SECRET_LEN: usize = 32;

    pub fn keypair() -> Result<([u8; PUBLIC_KEY_LEN], [u8; SECRET_KEY_LEN]), &'static str> {
        soshal_pqc_core::hybrid::hybrid_keygen_bytes().map_err(|_| "hybrid keygen failed")
    }

    pub fn encapsulate(
        pk: &[u8; PUBLIC_KEY_LEN],
        domain: &[u8],
    ) -> Result<([u8; CIPHERTEXT_LEN], [u8; SHARED_SECRET_LEN]), &'static str> {
        soshal_pqc_core::hybrid::hybrid_encapsulate_bytes(pk, domain)
            .map_err(|_| "invalid hybrid public key")
    }

    pub fn decapsulate(
        sk: &[u8; SECRET_KEY_LEN],
        ct: &[u8; CIPHERTEXT_LEN],
        domain: &[u8],
    ) -> Result<[u8; SHARED_SECRET_LEN], &'static str> {
        soshal_pqc_core::hybrid::hybrid_decapsulate_bytes(ct, sk, domain)
            .map_err(|_| "invalid hybrid ciphertext")
    }
}

pub mod kem {
    use super::*;

    pub const SEED_LEN: usize = 64;
    pub const PUBLIC_KEY_LEN: usize = 1184;
    pub const CIPHERTEXT_LEN: usize = 1088;
    pub const SHARED_SECRET_LEN: usize = 32;

    pub fn keypair() -> Result<([u8; PUBLIC_KEY_LEN], [u8; SEED_LEN]), &'static str> {
        pqc_kem::kem_keygen_bytes().map_err(|_| "kem keygen failed")
    }

    pub fn encapsulate(
        pk: &[u8; PUBLIC_KEY_LEN],
    ) -> Result<([u8; CIPHERTEXT_LEN], [u8; SHARED_SECRET_LEN]), &'static str> {
        pqc_kem::kem_encapsulate_bytes(pk).map_err(|_| "invalid public key")
    }

    pub fn decapsulate(
        sk: &[u8; SEED_LEN],
        ct: &[u8; CIPHERTEXT_LEN],
    ) -> Result<[u8; SHARED_SECRET_LEN], &'static str> {
        pqc_kem::kem_decapsulate_bytes(ct, sk).map_err(|_| "invalid ciphertext")
    }
}

pub mod dsa {
    use super::*;

    pub const SEED_LEN: usize = 32;
    pub const PUBLIC_KEY_LEN: usize = 1952;
    pub const SIGNATURE_LEN: usize = 3309;

    #[deprecated(note = "prefer soshal_pqc_core::dsa")]
    pub fn keypair_from_seed(seed: &[u8; SEED_LEN]) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
        let (sk_hex, pk_hex) = pqc_dsa::dsa_keygen(Some(seed)).map_err(|_| "dsa keygen failed")?;
        let pk = hex_decode(&pk_hex).map_err(|_| "invalid pk")?;
        let sk = hex_decode(&sk_hex).map_err(|_| "invalid sk")?;
        Ok((pk, sk))
    }

    #[deprecated(note = "prefer soshal_pqc_core::dsa")]
    pub fn keypair() -> Result<(Vec<u8>, Vec<u8>), &'static str> {
        let (sk_hex, pk_hex) = pqc_dsa::dsa_keygen(None).map_err(|_| "dsa keygen failed")?;
        let pk = hex_decode(&pk_hex).map_err(|_| "invalid pk")?;
        let sk = hex_decode(&sk_hex).map_err(|_| "invalid sk")?;
        Ok((pk, sk))
    }

    #[deprecated(note = "prefer soshal_pqc_core::dsa")]
    pub fn sign(sk_bytes: &[u8; SEED_LEN], msg: &[u8]) -> Result<Vec<u8>, &'static str> {
        pqc_dsa::dsa_sign_bytes(msg, sk_bytes).ok_or("signing failed")
    }

    #[deprecated(note = "prefer soshal_pqc_core::dsa")]
    pub fn verify(pk_bytes: &[u8], msg: &[u8], sig_bytes: &[u8]) -> Result<(), &'static str> {
        if pqc_dsa::dsa_verify_bytes(sig_bytes, msg, pk_bytes) {
            Ok(())
        } else {
            Err("verification failed")
        }
    }
}
