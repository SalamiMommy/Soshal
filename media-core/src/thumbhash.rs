//! ThumbHash encode/decode: tiny (~20-30 byte) blurred image fingerprints
//! sent inside post metadata so the UI can render a placeholder instantly
//! while the real image streams in from the mesh.
//!
//! Encode runs off the UI thread (Rust), decode produces a small RGBA buffer
//! that Flutter paints directly — never touches the UI isolate's decoder.

use crate::decoder::{fit_dimensions, DecodedRgbaFrame};
use image::GenericImageView;

/// Max edge used for hashing — ThumbHash is designed for ≤100px inputs.
const HASH_MAX_SIZE: u32 = 100;
/// Max bytes accepted as compressed image input to avoid memory exhaustion attacks.
pub const MAX_THUMBHASH_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Max pixels decoded to produce a ≤100 px hash.
///
/// The byte cap above bounds the *encoded* input, which says nothing about the
/// decoded size — a few hundred KB of PNG can expand to hundreds of MB of
/// pixels. 40 MPix is ~160 MB at RGBA, comfortably above any real photo at the
/// resolutions a feed serves, and far below the decode that would actually hurt.
/// The decoder's own `Limits` are not set here, so this is the check that bounds
/// the allocation.
const MAX_THUMBHASH_DECODE_PIXELS: u64 = 40_000_000;
/// Maximum valid ThumbHash binary length (spec standard is ~21-30 bytes).
pub const MAX_THUMBHASH_LEN: usize = 64;

/// Encodes a ThumbHash from compressed image bytes (downscaled first).
pub fn encode_thumbhash_from_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || bytes.len() > MAX_THUMBHASH_INPUT_BYTES {
        return Err("image input out of bounds".to_string());
    }
    // Header dimensions first, before any pixels are allocated.
    //
    // `load_from_memory` (what this used to call) decodes the whole image — up to
    // the 64 MiB `MAX_THUMBHASH_INPUT_BYTES` cap — and then throws nearly all
    // of it away into a <=100 px hash. That byte cap bounds the *encoded* input
    // and says nothing about the decoded size: a few hundred KB of PNG expands
    // to hundreds of MB of pixels, so the whole cost of this function was one
    // enormous decode to produce ~30 bytes of output. The pixel count is
    // knowable from the header for every format the crate supports, so it is
    // checked before the decode rather than after it.
    //
    // The reader is rebuilt rather than reused because `into_dimensions` takes
    // it by value; that is a second header parse, which is not the cost worth
    // optimising here.
    let (hdr_w, hdr_h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("thumbhash inspect: {e}"))?
        .into_dimensions()
        .map_err(|e| format!("thumbhash dimensions: {e}"))?;
    if hdr_w == 0 || hdr_h == 0 {
        return Err("thumbhash: degenerate image dimensions".to_string());
    }
    // `u64` throughout: an 8-byte-per-pixel image at the format's dimension
    // maximum overflows a `u32` byte count, and this must not be what wraps.
    if u64::from(hdr_w) * u64::from(hdr_h) > MAX_THUMBHASH_DECODE_PIXELS {
        return Err(format!(
            "thumbhash: image too large to decode ({hdr_w}x{hdr_h})"
        ));
    }

    // Convert to RGBA once, up front, and drop the decoder's own buffer
    // immediately: the hash needs RGBA and nothing else, so holding a
    // `DynamicImage` (whose variant may be RGB8, RGB16, Luma, …) alongside a
    // scaled copy of it is two full-size buffers for no benefit. Scaling from
    // the RGBA buffer keeps the peak at the decoded RGBA plus the <=100 px
    // result.
    let decoded = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("thumbhash inspect: {e}"))?
        .decode()
        .map_err(|e| format!("thumbhash decode source: {e}"))?;
    let (orig_w, orig_h) = decoded.dimensions();
    let src = decoded.into_rgba8();

    let (sw, sh, rgba) = if orig_w > HASH_MAX_SIZE || orig_h > HASH_MAX_SIZE {
        // `imageops::thumbnail` **stretches** to whatever size it is given,
        // despite the name — `DynamicImage::thumbnail` is the one that fits
        // inside the box, and it needs a `DynamicImage`, the buffer this rewrite
        // is avoiding. So the fit is computed and the exact fitted size handed to
        // the axis-aligned resize, which is byte-for-byte what
        // `DynamicImage::thumbnail` did.
        //
        // Byte-identical output matters more here than anywhere else: the
        // encoder's rounding and filter are baked into every thumbhash already
        // stored in post metadata.
        let (sw, sh) = fit_dimensions(orig_w, orig_h, HASH_MAX_SIZE, HASH_MAX_SIZE);
        let scaled = image::imageops::thumbnail(&src, sw, sh);
        (sw, sh, scaled.into_raw())
    } else {
        let (w, h) = src.dimensions();
        (w, h, src.into_raw())
    };
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
    if hash.is_empty() || hash.len() > MAX_THUMBHASH_LEN {
        return Err("invalid thumbhash length".to_string());
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
        assert!(decode_thumbhash_to_rgba(&[0u8; 100]).is_err());
        assert!(encode_thumbhash_from_bytes(&[]).is_err());
    }

    /// A 34-byte GIF declaring 20000x20000 — 400 MPix, or ~1.6 GB once decoded to
    /// RGBA — with no real image data behind it at all.
    ///
    /// This exists to pin *where* the pixel cap is enforced. The cap used to be
    /// implicit in the decode, so a header like this was bounded only by whatever
    /// the decoder felt like allocating; now the dimensions are read from the
    /// header first and the image is refused before a single pixel is decoded.
    /// The GIF is the cheapest real format for this: its logical screen
    /// descriptor carries the dimensions in the first 13 bytes, so the fixture is
    /// 34 bytes rather than the gigabytes the declared size implies. BMP would
    /// serve the same purpose but `image` is built without the `bmp` feature.
    #[test]
    fn oversized_image_is_refused_from_the_header_before_any_decode() {
        let header = gif_header(20_000, 20_000);
        assert_eq!(header.len(), 34, "fixture must be header-only");
        let err = encode_thumbhash_from_bytes(&header).expect_err("must refuse");
        assert!(
            err.contains("too large to decode"),
            "expected the pre-decode pixel cap, got: {err}"
        );
        assert!(
            !err.contains("decode source"),
            "the cap must fire before the decode is attempted, got: {err}"
        );
    }

    /// The byte cap and the pixel cap bound different things and both are needed.
    /// A 64 MiB input is under the byte cap, so nothing about the *input* size
    /// would stop it — only the header can. And the header check must not refuse
    /// sizes that are legitimately decodable, or it would be a regression rather
    /// than a bound.
    #[test]
    fn the_pixel_cap_rejects_only_past_the_limit() {
        // At the cap: 40 MPix exactly, which must be let through to the decode.
        let at_cap = encode_thumbhash_from_bytes(&gif_header(8_000, 5_000));
        assert!(
            matches!(&at_cap, Err(e) if e.contains("decode source")),
            "40 MPix is exactly at the cap and must reach the decode, got: {at_cap:?}"
        );

        // One pixel over: refused, and refused on size rather than at the decode.
        let over = encode_thumbhash_from_bytes(&gif_header(8_000, 5_001));
        assert!(
            matches!(&over, Err(e) if e.contains("too large to decode")),
            "40 MPix + 1 must be refused, got: {over:?}"
        );

        // A 1x1 image is small enough to pass both bounds and reach the decode;
        // the degenerate-dimension check must not fire on it.
        let tiny = encode_thumbhash_from_bytes(&gif_header(1, 1));
        assert!(
            !matches!(&tiny, Err(e) if e.contains("degenerate")),
            "1x1 is degenerate-looking but valid, got: {tiny:?}"
        );
    }

    /// Minimal GIF: a 13-byte header + logical screen descriptor, a global colour
    /// table, an image descriptor repeating the dimensions, and a one-byte LZW
    /// data block. `image` reads the dimensions from this alone, which is the
    /// whole point — a declared size never has to be materialised to be checked.
    fn gif_header(w: u16, h: u16) -> Vec<u8> {
        let mut f = Vec::with_capacity(34);
        f.extend_from_slice(b"GIF89a");
        f.extend_from_slice(&w.to_le_bytes());
        f.extend_from_slice(&h.to_le_bytes());
        f.extend_from_slice(&[0xF0, 0x00, 0x00]); // global colour table, 2 entries
        f.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        f.push(0x2C); // image descriptor
        f.extend_from_slice(&0u16.to_le_bytes()); // left
        f.extend_from_slice(&0u16.to_le_bytes()); // top
        f.extend_from_slice(&w.to_le_bytes());
        f.extend_from_slice(&h.to_le_bytes());
        f.push(0x00); // no local colour table
        f.push(0x02); // LZW minimum code size
        f.push(0x01); // one data sub-block
        f.push(0x00); // ...holding one byte
        f.push(0x00); // sub-block terminator
        f.push(0x3B); // trailer
        f
    }
}
