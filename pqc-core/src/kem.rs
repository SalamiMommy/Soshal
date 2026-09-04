use ml_kem::kem::Decapsulate;
use ml_kem::{DecapsulationKey768, EncapsulationKey768, MlKem768, Seed};
use ml_kem::{Encapsulate, KeyExport, TryKeyInit};
use zeroize::Zeroize;

pub const KEM_PK_LEN: usize = 1184;
pub const KEM_CT_LEN: usize = 1088;
pub const KEM_SS_LEN: usize = 32;
pub const KEM_SEED_LEN: usize = 64;

pub fn kem_keygen_bytes() -> Result<([u8; KEM_PK_LEN], [u8; KEM_SEED_LEN]), String> {
    let mut seed_bytes = [0u8; KEM_SEED_LEN];
    getrandom::fill(&mut seed_bytes).map_err(|e| format!("rng: {}", e))?;
    let seed_arr = Seed::try_from(seed_bytes.as_slice()).map_err(|e| format!("seed: {}", e))?;
    let seed_arr = zeroize::Zeroizing::new(seed_arr);
    let dk = DecapsulationKey768::from(*seed_arr);
    let ek = dk.encapsulation_key();
    let ek_bytes = ek.to_bytes();
    let mut pk = [0u8; KEM_PK_LEN];
    pk.copy_from_slice(ek_bytes.as_slice());
    Ok((pk, seed_bytes))
}

pub fn kem_encapsulate_bytes(
    pk_bytes: &[u8; KEM_PK_LEN],
) -> Result<([u8; KEM_CT_LEN], [u8; KEM_SS_LEN]), String> {
    let ek = EncapsulationKey768::new_from_slice(pk_bytes).map_err(|_| "bad ek".to_string())?;
    let (ct, mut ss) = ek.encapsulate_with_rng(&mut rand_core::UnwrapErr(getrandom::SysRng));
    let mut ct_arr = [0u8; KEM_CT_LEN];
    ct_arr.copy_from_slice(ct.as_slice());
    let mut ss_arr = [0u8; KEM_SS_LEN];
    ss_arr.copy_from_slice(ss.as_slice());
    ss.zeroize();
    Ok((ct_arr, ss_arr))
}

pub fn kem_decapsulate_bytes(
    ct_bytes: &[u8; KEM_CT_LEN],
    sk_bytes: &[u8; KEM_SEED_LEN],
) -> Result<[u8; KEM_SS_LEN], String> {
    let seed_arr = Seed::try_from(&sk_bytes[..]).map_err(|_| "bad seed".to_string())?;
    let dk = DecapsulationKey768::from(seed_arr);
    let ct = ml_kem::Ciphertext::<MlKem768>::try_from(&ct_bytes[..])
        .map_err(|_| "bad ct".to_string())?;
    let mut ss = dk.decapsulate(&ct);
    let mut ss_arr = [0u8; KEM_SS_LEN];
    ss_arr.copy_from_slice(ss.as_slice());
    ss.zeroize();
    Ok(ss_arr)
}

/// Generates a new ML-KEM-768 keypair, returning `(pk_hex, sk_hex)`.
pub fn kem_keygen() -> Result<(String, String), String> {
    let (pk, mut sk) = kem_keygen_bytes()?;
    let res = (hex::encode(pk), hex::encode(sk));
    sk.zeroize();
    Ok(res)
}

pub fn kem_encapsulate(peer_pk_hex: &str) -> Result<(String, String), String> {
    let pk_bytes = hex::decode(peer_pk_hex).map_err(|_| "bad pk hex".to_string())?;
    if pk_bytes.len() != KEM_PK_LEN {
        return Err("bad pk len".to_string());
    }
    let mut pk_arr = [0u8; KEM_PK_LEN];
    pk_arr.copy_from_slice(&pk_bytes);
    let (ct, mut ss) = kem_encapsulate_bytes(&pk_arr)?;
    let res = (hex::encode(ct), hex::encode(ss));
    ss.zeroize();
    Ok(res)
}

pub fn kem_decapsulate(ct_hex: &str, sk_hex: &str) -> Result<String, String> {
    let ct_bytes = hex::decode(ct_hex).map_err(|_| "bad ct hex".to_string())?;
    let sk_bytes =
        zeroize::Zeroizing::new(hex::decode(sk_hex).map_err(|_| "bad sk hex".to_string())?);
    if ct_bytes.len() != KEM_CT_LEN || sk_bytes.len() != KEM_SEED_LEN {
        return Err("bad input length".to_string());
    }
    let mut ct_arr = [0u8; KEM_CT_LEN];
    let mut sk_arr = [0u8; KEM_SEED_LEN];
    ct_arr.copy_from_slice(&ct_bytes);
    sk_arr.copy_from_slice(&sk_bytes);
    let res = kem_decapsulate_bytes(&ct_arr, &sk_arr);
    sk_arr.zeroize();
    let mut ss = res?;
    let res = hex::encode(ss);
    ss.zeroize();
    Ok(res)
}
