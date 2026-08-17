//! Background RGBA image decoding engine.
//! Decodes compressed image formats (WebP, PNG, JPEG, GIF) into uncompressed
//! 32-bit RGBA pixel buffers off the UI isolate thread.

use image::{GenericImageView, ImageFormat};

#[derive(Debug, Clone)]
pub struct DecodedRgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decode raw image bytes into an uncompressed RGBA pixel buffer.
/// Optionally resizes to target_width/target_height while preserving aspect ratio.
pub fn decode_to_rgba(
    bytes: &[u8],
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> Result<DecodedRgbaFrame, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("Failed to decode image from memory: {e}"))?;

    let resized_img = match (max_width, max_height) {
        (Some(mw), Some(mh)) if mw > 0 && mh > 0 => {
            let (orig_w, orig_h) = img.dimensions();
            if orig_w > mw || orig_h > mh {
                img.resize(mw, mh, image::imageops::FilterType::Triangle)
            } else {
                img
            }
        }
        _ => img,
    };

    let (width, height) = resized_img.dimensions();
    let rgba_img = resized_img.into_rgba8();
    let pixels = rgba_img.into_raw();

    Ok(DecodedRgbaFrame {
        width,
        height,
        pixels,
    })
}

/// Helper to infer image format from byte header or filename
pub fn detect_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    image::guess_format(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_image_decode_fails_gracefully() {
        let res = decode_to_rgba(&[], None, None);
        assert!(res.is_err());
    }
}
