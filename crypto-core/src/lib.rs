//! Cryptography core: NIP-44 v2 messaging, at-rest encryption, PQC
//! (ML-KEM/ML-DSA) + hybrid ratchet, key derivation, hashing/hardware
//! acceleration, FROST scaffolding, and zero-knowledge trust helpers.

pub mod at_rest;
pub mod base64;
pub mod base64url;
pub mod frost;
pub mod hardware_accel;
pub mod hash;
pub mod key_derivation;
pub mod lan;
pub mod nip44;
pub mod pir;
pub mod pqc;
pub mod pqc_ratchet;
pub mod zk_trust;
