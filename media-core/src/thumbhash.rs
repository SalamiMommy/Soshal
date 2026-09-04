//! ThumbHash encode/decode: tiny (~20-30 byte) blurred image fingerprints
//! sent inside post metadata so the UI can render a placeholder instantly
//! while the real image streams in from the mesh.
//!
//! Encode runs off the UI thread (Rust), decode produces a small RGBA buffer
//! that Flutter paints directly — never touches the UI isolate's decoder.

use crate::decoder::DecodedRgbaFrame;
use image::GenericImageView;

/// Max edge used for hashing — ThumbHash is designed for ≤100px inputs.
const HASH_MAX_SIZE: u32 = 100;

/// Encodes a ThumbHash from compressed image bytes (downscaled first).
pub fn encode_thumbhash_from_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img =
        image::load_from_memory(bytes).map_err(|e| format!("thumbhash decode source: {e}"))?;
    let (w, h) = img.dimensions();
    let scaled = if w > HASH_MAX_SIZE || h > HASH_MAX_SIZE {
        img.thumbnail(HASH_MAX_SIZE, HASH_MAX_SIZE)
    } else {
        img
    };
    let (sw, sh) = scaled.dimensions();
    let rgba = scaled.into_rgba8().into_raw();
    Ok(thumbhash::rgba_to_thumb_hash(
        sw as usize,
        sh as usize,
        &rgba,
    ))
}

/// Encodes a ThumbHash from an already-decoded RGBA buffer.
pub fn encode_thumbhash_from_rgba(
    width: usize,
    height: usize,
    rgba: &[u8],
) -> Result<Vec<u8>, String> {
    let cap = match width.checked_mul(height).and_then(|v| v.checked_mul(4)) {
        Some(c) => c,
        None => return Err("dimensions overflow".to_string()),
    };
    if rgba.len() < cap {
        return Err("rgba buffer too small".to_string());
    }
    Ok(thumbhash::rgba_to_thumb_hash(width, height, rgba))
}

/// Decodes a ThumbHash into a small RGBA frame (its intrinsic size).
pub fn decode_thumbhash_to_rgba(hash: &[u8]) -> Result<DecodedRgbaFrame, String> {
    if hash.is_empty() {
        return Err("empty thumbhash".to_string());
    }
    let (w, h, rgba) =
        thumbhash::thumb_hash_to_rgba(hash).map_err(|_| "invalid thumbhash".to_string())?;
    Ok(DecodedRgbaFrame {
        width: w as u32,
        height: h as u32,
        pixels: rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_rgba_hash() {
        let w = 64usize;
        let h = 48usize;
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let r = (i % 251) as u8;
                let g = (i % 199) as u8;
                let b = (i % 157) as u8;
                [r, g, b, 255u8]
            })
            .collect();
        let hash = encode_thumbhash_from_rgba(w, h, &rgba).unwrap();
        assert!(!hash.is_empty());
        assert!(hash.len() <= 40);
        let frame = decode_thumbhash_to_rgba(&hash).unwrap();
        assert_eq!(
            frame.pixels.len(),
            frame.width as usize * frame.height as usize * 4
        );
    }

    #[test]
    fn empty_hash_rejected() {
        assert!(decode_thumbhash_to_rgba(&[]).is_err());
    }
}
