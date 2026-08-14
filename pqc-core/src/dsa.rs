use ml_dsa::{KeyExport, Keypair};
use zeroize::Zeroizing;

pub fn dsa_keygen(seed: Option<&[u8]>) -> Result<(String, String), String> {
    let seed_bytes: [u8; 32] = match seed {
        Some(s) if s.len() >= 32 => {
            let mut r = [0u8; 32];
            r.copy_from_slice(&s[..32]);
            r
        }
        _ => {
            let mut r = [0u8; 32];
            getrandom::fill(&mut r).map_err(|e| format!("rng: {}", e))?;
            r
        }
    };
    let seed_arr = ml_dsa::Seed::try_from(&seed_bytes[..]).map_err(|_| "bad seed".to_string())?;
    let seed_arr = Zeroizing::new(seed_arr);
    let sk = ml_dsa::SigningKey::<ml_dsa::MlDsa65>::from_seed(&seed_arr);
    let vk = sk.verifying_key();
    let sk_bytes = Zeroizing::new(sk.to_bytes());
    let vk_bytes = vk.to_bytes();
    Ok((hex::encode(&sk_bytes), hex::encode(vk_bytes)))
}

pub fn dsa_sign_bytes(msg: &[u8], sk_bytes: &[u8]) -> Option<Vec<u8>> {
    let signer = crate::compat::make_signer_from_secret_bytes(sk_bytes)?;
    Some(signer(msg).to_vec())
}

pub fn dsa_sign(msg: &[u8], sk_hex: &str) -> Option<String> {
    let sk_bytes = Zeroizing::new(hex::decode(sk_hex).ok()?);
    dsa_sign_bytes(msg, &sk_bytes).map(|sig| hex::encode(&sig))
}

pub fn dsa_verify_bytes(sig_bytes: &[u8], msg: &[u8], pk_bytes: &[u8]) -> bool {
    use ml_dsa::signature::Verifier;
    let sig = match ml_dsa::Signature::<ml_dsa::MlDsa65>::try_from(sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let vk_arr = match ml_dsa::EncodedVerifyingKey::<ml_dsa::MlDsa65>::try_from(pk_bytes) {
        Ok(arr) => arr,
        Err(_) => return false,
    };
    let vk = ml_dsa::VerifyingKey::<ml_dsa::MlDsa65>::decode(&vk_arr);
    vk.verify(msg, &sig).is_ok()
}

pub fn dsa_verify(sig_bytes: &[u8], msg: &[u8], pk_hex: &str) -> bool {
    let pk_bytes = match hex::decode(pk_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };
    dsa_verify_bytes(sig_bytes, msg, &pk_bytes)
}

pub fn dsa_verify_hex(sig_hex: &str, msg: &[u8], pk_hex: &str) -> bool {
    let sig_bytes = match hex::decode(sig_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };
    dsa_verify(&sig_bytes, msg, pk_hex)
}
