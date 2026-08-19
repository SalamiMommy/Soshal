//! Authentication FFI module
//!
//! Handles key generation, mnemonic operations, and identity management.

use flutter_rust_bridge::frb;
use nostr::nips::nip19::{FromBech32, ToBech32};
use serde::{Deserialize, Serialize};
use soshal_identity_core::mnemonic::{generate_mnemonic, restore_from_mnemonic, validate_mnemonic};
use soshal_nostr_core::keys::{from_nsec, generate_keys};
use zeroize::{Zeroize, Zeroizing};

/// Result wrapper for FFI operations.
///
/// Thin alias over `Result<T, String>` so flutter_rust_bridge maps errors to
/// Generate a new keypair and unlock the signer
#[frb(sync, serialize)]
pub fn auth_generate_keypair() -> Result<String, String> {
    let keys = generate_keys();
    let nsec = Zeroizing::new(keys.secret_key().to_secret_hex());
    let pk = super::signer::signer_unlock((*nsec).to_string())?;
    super::util::json_ok(KeyPairResult {
        public_key: pk,
        secret_key: nsec,
    })
}

/// Key generation result
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KeyPairResult {
    pub public_key: String,
    pub secret_key: Zeroizing<String>,
}

/// Generate a new BIP-39 mnemonic phrase
#[frb(sync, serialize)]
pub fn auth_generate_mnemonic() -> Result<String, String> {
    generate_mnemonic().into()
}

/// Validate a BIP-39 mnemonic phrase
#[frb(sync, serialize)]
pub fn auth_validate_mnemonic(mnemonic: String) -> Result<bool, String> {
    Ok(validate_mnemonic(&mnemonic)).into()
}

/// Restore a keypair from a BIP-39 mnemonic and unlock the in-process signer.
/// The secret key is NOT returned across FFI — the signer is already unlocked
/// in-process. Only the hex public key is returned.
#[frb(serialize)]
pub async fn auth_restore_from_mnemonic(
    mut mnemonic: String,
    mut passphrase: String,
) -> Result<String, String> {
    let res = restore_from_mnemonic(&mnemonic, &passphrase);
    mnemonic.zeroize();
    passphrase.zeroize();
    let mut keys = res.map_err(super::util::to_err)?;
    let nsec = Zeroizing::new(keys.private_key_hex.clone());
    let pk = super::signer::signer_unlock((*nsec).to_string())?;
    keys.private_key_hex.zeroize();
    // Return only the public key; the signer now holds the unlocked identity.
    super::util::json_ok(KeyPairResult {
        public_key: pk,
        secret_key: Zeroizing::new(String::new()),
    })
}

/// Get public key from nsec (bech32 encoded secret key)
#[frb(sync, serialize)]
pub fn auth_public_key_from_nsec(mut nsec: String) -> Result<String, String> {
    let res = from_nsec(&nsec)
        .map_err(super::util::to_err)
        .map(|keys| keys.public_key().to_string());
    nsec.zeroize();
    res.into()
}

/// Encode public key as npub (bech32)
#[frb(sync, serialize)]
pub fn auth_npub_encode(public_key: String) -> Result<String, String> {
    use nostr::key::PublicKey;
    PublicKey::from_hex(&public_key)
        .map_err(super::util::to_err)
        .and_then(|pk| pk.to_bech32().map_err(super::util::to_err))
        .into()
}

/// Decode npub to hex public key
#[frb(sync, serialize)]
pub fn auth_npub_decode(npub: String) -> Result<String, String> {
    use nostr::key::PublicKey;
    PublicKey::from_bech32(&npub)
        .map_err(super::util::to_err)
        .map(|pk| pk.to_hex())
        .into()
}
