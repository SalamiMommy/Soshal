use ring::{digest, hmac};

/// BLAKE3 hash (SIMD-accelerated: NEON on ARM, AVX2/AVX-512 on x86). Used for
/// content-defined chunk identities and swarm chunk verification.
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(blake3::hash(data).as_bytes());
    out
}

pub fn blake3_hash_hex(data: &[u8]) -> String {
    hex::encode(blake3_hash(data))
}

pub fn blake3_hash_stream<R: std::io::Read>(mut reader: R) -> Result<[u8; 32], std::io::Error> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest::digest(&digest::SHA256, data).as_ref());
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

pub fn sha256_stream<R: std::io::Read>(mut reader: R) -> Result<[u8; 32], std::io::Error> {
    let mut ctx = digest::Context::new(&digest::SHA256);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    Ok(out)
}

pub fn sha256_stream_hex<R: std::io::Read>(reader: R) -> Result<String, std::io::Error> {
    sha256_stream(reader).map(hex::encode)
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut out = [0u8; 32];
    out.copy_from_slice(hmac::sign(&key, data).as_ref());
    out
}

pub fn hmac_sha256_slices(key: &[u8], slices: &[&[u8]]) -> [u8; 32] {
    let s_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut ctx = hmac::Context::with_key(&s_key);
    for slice in slices {
        ctx.update(slice);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.sign().as_ref());
    out
}

pub fn hkdf_sha256(
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    okm_len: usize,
) -> Result<Vec<u8>, String> {
    soshal_pqc_core::hkdf::hkdf_sha256(ikm, salt, info, okm_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_hash_hex_vectors() {
        let expected = hex::encode(blake3::hash(b"").as_bytes());
        assert_eq!(blake3_hash_hex(b""), expected);
        assert_eq!(
            blake3_hash_hex(b""),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_eq!(
            blake3_hash_hex(b"abc"),
            hex::encode(blake3::hash(b"abc").as_bytes())
        );
    }

    #[test]
    fn test_blake3_hash_hex_deterministic_64() {
        let a = blake3_hash_hex(b"payload");
        let b = blake3_hash_hex(b"payload");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(blake3_hash_hex(b"payload"), blake3_hash_hex(b"payload2"));
    }

    #[test]
    fn test_sha256_stream_hex_known_vector() {
        let digest = sha256_stream_hex(std::io::empty()).unwrap();
        assert_eq!(digest.len(), 64);
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_stream_hex_deterministic_distinct() {
        let a = sha256_stream_hex(std::io::Cursor::new(b"alpha")).unwrap();
        let b = sha256_stream_hex(std::io::Cursor::new(b"beta")).unwrap();
        let a2 = sha256_stream_hex(std::io::Cursor::new(b"alpha")).unwrap();
        assert_eq!(a, a2);
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
