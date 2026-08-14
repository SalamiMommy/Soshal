//! HKDF-SHA256 (RFC 5869) via ring — single HKDF implementation for the
//! workspace (pqc-core owns it; crypto-core delegates).

use ring::hkdf;

struct OkmLength(usize);

impl hkdf::KeyType for OkmLength {
    fn len(&self) -> usize {
        self.0
    }
}

pub fn hkdf_sha256(
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    okm_len: usize,
) -> Result<Vec<u8>, String> {
    let mut okm = vec![0u8; okm_len];
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, salt);
    let prk = salt.extract(ikm);
    let info_slices = [info];
    let okm_out = prk
        .expand(&info_slices, OkmLength(okm_len))
        .map_err(|e| format!("hkdf expand: {}", e))?;
    okm_out
        .fill(&mut okm)
        .map_err(|e| format!("hkdf fill: {}", e))?;
    Ok(okm)
}
