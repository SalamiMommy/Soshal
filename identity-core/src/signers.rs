//! Secret signer: the sole Rust-side access path to identity key material.
//!
//! All commands interact with identity secrets through [`SigningOps`]
//! implementations instead of holding `nostr::key::Keys` directly. The default
//! [`Signer`] keeps keys in-process; a remote signer (key agent subprocess)
//! can implement the same trait so key material never touches the app
//! process. Key material is erased on drop (scrubbing in `Keys::drop` +
//! `SecretKey::drop`), so long-lived clones are the exception, not the rule.

use std::sync::Arc;

use nostr::event::{Event, EventBuilder, FinalizeUnsignedEvent, SignEvent, UnsignedEvent};
use nostr::key::{Keys, PublicKey};
use zeroize::Zeroize;

/// Narrow signing surface shared by in-process and remote (key agent)
/// signers. Key material is never part of this interface: implementations
/// expose public key, signature and ciphertext outputs only.
pub trait SigningOps: Send + Sync + std::fmt::Debug {
    /// Public key (hex) — public, safe to cache.
    fn public_key_hex(&self) -> String;

    /// Public key — public, safe to cache.
    fn public_key(&self) -> Result<PublicKey, String>;

    /// Build (current timestamp) and sign an event builder.
    fn sign_builder(&self, builder: EventBuilder) -> Result<Event, String>;

    /// Sign a raw unsigned-event JSON payload, returning signed-event JSON.
    fn sign_event_json(&self, event_json: &str) -> Result<String, String>;

    /// Schnorr-sign a 32-byte digest under domain separation for `purpose`.
    /// Only purposes listed in the built-in allowlist are accepted; any other
    /// value returns an error so a compromised caller cannot coerce the signer
    /// into signing arbitrary domains.
    fn sign_schnorr_digest(&self, digest: &[u8], purpose: &str) -> Result<String, String>;

    /// NIP-44 encrypt (v2) to a recipient public key.
    fn nip44_encrypt(&self, to: &PublicKey, content: &str) -> Result<String, String>;

    /// NIP-44 decrypt (v2) from a sender public key.
    fn nip44_decrypt(&self, from: &PublicKey, payload: &str) -> Result<String, String>;
}

/// In-process signer: wraps identity keys, exposing only [`SigningOps`].
///
/// Cloning shares the same key material (same `Keys` clone); prefer to keep
/// a single signer per session and clone only to hand into `nostr_sdk`.
#[derive(Debug, Clone)]
pub struct Signer {
    keys: Keys,
}

impl Signer {
    /// Wrap secret key material for signing.
    pub fn new(keys: Keys) -> Self {
        Signer { keys }
    }

    /// Sign an already-built unsigned event.
    pub fn sign(&self, unsigned: UnsignedEvent) -> Result<Event, String> {
        self.keys
            .sign_event(unsigned)
            .map_err(|e| format!("signing error: {}", e))
    }
}

fn shared_secret(sk: &nostr::key::SecretKey, pk: &PublicKey) -> Result<[u8; 32], String> {
    use secp256k1::{ecdh, Parity, PublicKey as SecpPublicKey};
    let xonly = pk.xonly().map_err(|e| format!("pubkey: {e}"))?;
    let secp_pk = SecpPublicKey::from_x_only_public_key(xonly, Parity::Even);
    // Raw ECDH point (x ‖ y, 64 bytes); take the x-coordinate. The default
    // SharedSecret hash is SHA-256 of the *compressed* point, which embeds the
    // parity bit — that would make the derived key depend on each party's y
    // parity (lost in x-only pubkeys) and break encryption ~half the time.
    let mut xy = ecdh::shared_secret_point(&secp_pk, sk);
    let mut key = [0u8; 32];
    key.copy_from_slice(&xy[..32]);
    xy.zeroize();
    Ok(key)
}

impl SigningOps for Signer {
    fn public_key_hex(&self) -> String {
        self.keys.public_key().to_string()
    }

    fn public_key(&self) -> Result<PublicKey, String> {
        Ok(self.keys.public_key())
    }

    fn sign_builder(&self, builder: EventBuilder) -> Result<Event, String> {
        let unsigned = builder.finalize_unsigned(self.keys.public_key());
        self.sign(unsigned)
    }

    fn sign_event_json(&self, event_json: &str) -> Result<String, String> {
        let unsigned: UnsignedEvent =
            serde_json::from_str(event_json).map_err(|e| format!("parse unsigned event: {e}"))?;
        let signed = self.sign(unsigned)?;
        serde_json::to_string(&signed).map_err(|e| format!("serialize: {e}"))
    }

    fn sign_schnorr_digest(&self, digest: &[u8], purpose: &str) -> Result<String, String> {
        if digest.len() != 32 {
            return Err(format!("digest must be 32 bytes, got {}", digest.len()));
        }
        // H-2 fix: allowlist and domain-separate Schnorr signing purposes.
        const ALLOWED_PURPOSES: &[&str] = &["blossom-auth"];
        if !ALLOWED_PURPOSES.contains(&purpose) {
            return Err(format!("unknown schnorr purpose: {purpose}"));
        }
        // sha256(purpose_bytes || digest) so the same raw digest cannot be
        // reused across different signing contexts.
        let mut preimage = Vec::with_capacity(purpose.len() + 32);
        preimage.extend_from_slice(purpose.as_bytes());
        preimage.extend_from_slice(digest);
        let separated_digest = ring::digest::digest(&ring::digest::SHA256, &preimage);
        let separated_bytes: [u8; 32] = separated_digest
            .as_ref()
            .try_into()
            .map_err(|_| "sha256 output wrong length".to_string())?;
        Ok(self.keys.sign_schnorr(separated_bytes).to_string())
    }

    fn nip44_encrypt(&self, to: &PublicKey, content: &str) -> Result<String, String> {
        let key = shared_secret(self.keys.secret_key(), to)?;
        soshal_crypto_core::nip44::encrypt(content.as_bytes(), &key)
            .map_err(|e| format!("encrypt: {e}"))
    }

    fn nip44_decrypt(&self, from: &PublicKey, payload: &str) -> Result<String, String> {
        let key = shared_secret(self.keys.secret_key(), from)?;
        let plaintext = soshal_crypto_core::nip44::decrypt(payload, &key)
            .map_err(|e| format!("decrypt: {e}"))?;
        String::from_utf8(plaintext).map_err(|e| format!("utf8: {e}"))
    }
}

/// Shared reference to any signer implementation (in-process or the key
/// agent subprocess), held across the app. Derefs to the [`SigningOps`] trait
/// object so the rest of the codebase can call signer methods directly.
#[derive(Clone)]
pub struct SignerHandle {
    inner: Arc<dyn SigningOps>,
}

impl SignerHandle {
    /// Wrap a signer implementation.
    pub fn new(inner: Arc<dyn SigningOps>) -> Self {
        SignerHandle { inner }
    }

    /// Wrap an in-process signer built from key material.
    pub fn in_process(keys: Keys) -> Self {
        SignerHandle {
            inner: Arc::new(Signer::new(keys)),
        }
    }
}

impl std::ops::Deref for SignerHandle {
    type Target = dyn SigningOps;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

impl std::fmt::Debug for SignerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignerHandle")
            .field("pubkey_hex", &self.inner.public_key_hex())
            .finish()
    }
}

impl nostr::key::GetPublicKey for SignerHandle {
    type Error = nostr::error::Error;
    fn get_public_key(&self) -> Result<PublicKey, Self::Error> {
        self.inner.public_key().map_err(nostr::error::Error::other)
    }
}

impl nostr::key::AsyncGetPublicKey for SignerHandle {
    type Error = nostr::error::Error;
    fn get_public_key_async(
        &self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<PublicKey, Self::Error>> + Send + '_>,
    > {
        Box::pin(async move { self.inner.public_key().map_err(nostr::error::Error::other) })
    }
}

impl nostr::event::SignEvent for SignerHandle {
    type Error = nostr::error::Error;
    fn sign_event(&self, unsigned: UnsignedEvent) -> Result<Event, Self::Error> {
        let json = serde_json::to_string(&unsigned).map_err(nostr::error::Error::other)?;
        let signed = self
            .inner
            .sign_event_json(&json)
            .map_err(nostr::error::Error::other)?;
        serde_json::from_str(&signed).map_err(nostr::error::Error::other)
    }
}

impl nostr::event::AsyncSignEvent for SignerHandle {
    type Error = nostr::error::Error;
    fn sign_event_async(
        &self,
        unsigned: UnsignedEvent,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Event, Self::Error>> + Send + '_>>
    {
        Box::pin(async move {
            let json = serde_json::to_string(&unsigned).map_err(nostr::error::Error::other)?;
            let signed = self
                .inner
                .sign_event_json(&json)
                .map_err(nostr::error::Error::other)?;
            serde_json::from_str(&signed).map_err(nostr::error::Error::other)
        })
    }
}
