use image::{GenericImageView, ImageFormat, ImageReader, Limits};
use std::io::Cursor;

const MAX_DECODE_DIMENSION: u32 = 8192;
const MAX_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024; // 64 MB

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
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("Failed to inspect image format: {e}"))?;
    reader.limits(limits);

    let img = reader
        .decode()
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

    fn test_png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut img = image::RgbImage::new(width, height);
        for p in img.pixels_mut() {
            *p = image::Rgb([200u8, 30, 30]);
        }
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn test_decode_to_rgba_downscales_when_orig_exceeds_max() {
        let buf = test_png_bytes(200, 100);
        let frame = decode_to_rgba(&buf, Some(100), Some(50)).unwrap();
        assert_eq!((frame.width, frame.height), (100, 50));
        assert_eq!(frame.pixels.len(), 100 * 50 * 4);
    }

    #[test]
    fn test_decode_to_rgba_zero_max_falls_back_to_original() {
        let buf = test_png_bytes(200, 100);
        for (mw, mh) in [
            (Some(0), None),
            (None, Some(0)),
            (Some(0), Some(0)),
            (Some(0), Some(10)),
        ] {
            let frame = decode_to_rgba(&buf, mw, mh).unwrap();
            assert_eq!((frame.width, frame.height), (200, 100));
            assert_eq!(frame.pixels.len(), 200 * 100 * 4);
        }
    }

    #[test]
    fn test_decode_to_rgba_rejects_empty_bytes() {
        assert!(decode_to_rgba(&[], None, None).is_err());
    }
}
