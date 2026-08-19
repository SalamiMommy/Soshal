use bip32::XPrv;
use bip39::Mnemonic;
use nostr::key::Keys;
use nostr::key::SecretKey;
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroize;

const NOSTR_DERIVATION_PATH: &str = "m/44'/1237'/0'/0/0";

#[derive(Debug)]
pub struct MnemonicResult {
    pub private_key_hex: String,
    pub public_key_hex: String,
}

pub fn generate_mnemonic() -> Result<String, String> {
    let mut entropy = [0u8; 32];
    let rng = SystemRandom::new();
    let res = rng
        .fill(&mut entropy)
        .map_err(|e| format!("rng error: {}", e))
        .and_then(|_| {
            Mnemonic::from_entropy(&entropy).map_err(|e| format!("mnemonic error: {}", e))
        })
        .map(|m| m.to_string());
    entropy.zeroize();
    res
}

pub fn validate_mnemonic(phrase: &str) -> bool {
    Mnemonic::parse_normalized(phrase).is_ok()
}

/// Zeroizes the mnemonic seed on drop, including on early error returns —
/// the 64-byte seed is the master key of the mnemonic and must not linger.
struct SeedGuard([u8; 64]);

impl Drop for SeedGuard {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub fn restore_from_mnemonic(phrase: &str, passphrase: &str) -> Result<MnemonicResult, String> {
    let mnemonic =
        Mnemonic::parse_normalized(phrase).map_err(|e| format!("invalid mnemonic: {}", e))?;
    let seed = SeedGuard(mnemonic.to_seed(passphrase));

    let path: bip32::DerivationPath = NOSTR_DERIVATION_PATH
        .parse()
        .map_err(|e: bip32::Error| format!("invalid derivation path: {}", e))?;

    let xprv =
        XPrv::derive_from_path(seed.0, &path).map_err(|e| format!("derive from path: {}", e))?;

    let mut sk_bytes = xprv.private_key().to_bytes();
    let sk = SecretKey::from_slice(&sk_bytes).map_err(|e| format!("secret key from bytes: {}", e));
    sk_bytes.zeroize();
    let keys = Keys::new(sk?);

    // Only the account key (hex) is handed to the frontend; the mnemonic
    // master seed is zeroized by SeedGuard on every exit path.
    Ok(MnemonicResult {
        private_key_hex: keys.secret_key().to_secret_hex(),
        public_key_hex: keys.public_key().to_string(),
    })
}
