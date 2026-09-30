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

    // The only conversion this function ever needs is RGBA8, so convert once,
    // here, and drop the decoder's own buffer immediately. The previous shape
    // held a `DynamicImage` (whose variant may be RGB8, RGB16, Luma, …), then
    // built a second `DynamicImage` for the resize, then converted that to RGBA8
    // and copied again — so a bounded 64 MiB decode could sit alongside a
    // full-size RGBA copy alongside the resized result, and a resize computed
    // from the *decoded* pixels paid for all of it even though the caller only
    // ever sees the small one.
    let decoded = reader
        .decode()
        .map_err(|e| format!("Failed to decode image from memory: {e}"))?;
    let (orig_w, orig_h) = decoded.dimensions();
    let src = decoded.into_rgba8();
    // `src` is dropped as soon as the resize returns, so the peak is the decoded
    // RGBA plus the destination — never both plus a `DynamicImage` in between.
    let rgba = match (max_width, max_height) {
        (Some(mw), Some(mh)) if mw > 0 && mh > 0 && (orig_w > mw || orig_h > mh) => {
            // The fit has to be computed here rather than delegated, because
            // neither resize helper preserves the aspect ratio:
            //
            // - `DynamicImage::resize` does preserve it, but it operates on a
            //   `DynamicImage`, which is the buffer this rewrite exists to avoid
            //   holding a second copy of.
            // - `imageops::thumbnail` looks like the right call but **stretches**:
            //   it allocates exactly `new_width x new_height` and scales x and y
            //   independently, so a 200x100 source asked for 100x100 comes back
            //   100x100 — distorted. A test pins that.
            //
            // So: fit inside the box, then resize to the exact fitted size.
            let (tw, th) = fit_dimensions(orig_w, orig_h, mw, mh);
            image::imageops::resize(&src, tw, th, image::imageops::FilterType::Triangle)
        }
        _ => src,
    };

    let (width, height) = rgba.dimensions();
    Ok(DecodedRgbaFrame {
        width,
        height,
        pixels: rgba.into_raw(),
    })
}

/// Helper to infer image format from byte header or filename
pub fn detect_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    image::guess_format(bytes).ok()
}

/// Size that fits `orig` inside `max_w x max_h` with the aspect ratio preserved.
///
/// A local reimplementation of the `image` crate's own private
/// `math::utils::resize_dimensions(.., fill = false)`, down to the rounding and
/// the minimum of 1, because that is the function `DynamicImage::resize` and
/// `DynamicImage::thumbnail` used and its behaviour is what every caller of this
/// module already observes. Neither public resize helper can be used directly:
///
/// - `DynamicImage::{resize,thumbnail}` preserve the ratio, but they need a
///   `DynamicImage`, which is exactly the second full-size buffer this module
///   avoids holding.
/// - `imageops::{resize,thumbnail}` take a concrete buffer, which is what we
///   want, but **both stretch** to the size given — `thumbnail` despite its name
///   allocates exactly `new_width x new_height` and scales the axes
///   independently. Passing the caller's box straight through would silently
///   distort every image whose ratio differs from its box.
///
/// So the fit is computed here and the axis-aligned resize does the scaling. This
/// also keeps the output byte-identical to the previous implementation, which
/// matters for the thumbhash path: the encoder's filter and rounding are baked
/// into every hash already stored in post metadata.
///
/// The two `u32::MAX` clamp branches are unreachable for the decode path
/// (`MAX_DECODE_DIMENSION` caps the source at 8192), but a caller can still pass
/// `u32::MAX` as a target, so they are kept rather than assumed away.
pub(crate) fn fit_dimensions(orig_w: u32, orig_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let wratio = f64::from(max_w) / f64::from(orig_w);
    let hratio = f64::from(max_h) / f64::from(orig_h);
    let ratio = f64::min(wratio, hratio);

    let nw = ((f64::from(orig_w) * ratio).round() as u64).max(1);
    let nh = ((f64::from(orig_h) * ratio).round() as u64).max(1);

    if nw > u64::from(u32::MAX) {
        let ratio = f64::from(u32::MAX) / f64::from(orig_w);
        (
            u32::MAX,
            ((f64::from(orig_h) * ratio).round() as u32).max(1),
        )
    } else if nh > u64::from(u32::MAX) {
        let ratio = f64::from(u32::MAX) / f64::from(orig_h);
        (
            ((f64::from(orig_w) * ratio).round() as u32).max(1),
            u32::MAX,
        )
    } else {
        (nw as u32, nh as u32)
    }
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

    /// `decode_to_rgba` used to resize through `DynamicImage::resize`, which
    /// fits the image *inside* the box and keeps the aspect ratio. Converting to
    /// RGBA first and then calling `imageops::resize` looks equivalent and is
    /// not: that one stretches to exactly the size it is given. A 200x100 source
    /// asked for 100x100 would come back 100x100 — visibly distorted — where
    /// before it came back 100x50.
    ///
    /// This is the test that distinguishes "fits inside the box" from "stretches
    /// to the box", which no existing test did: they all asked for a box with
    /// the same ratio as the source.
    #[test]
    fn resize_fits_inside_the_box_instead_of_stretching_to_it() {
        let buf = test_png_bytes(200, 100);
        // Box is square; source is 2:1. Fitting must yield 100x50, stretching
        // would yield 100x100.
        let frame = decode_to_rgba(&buf, Some(100), Some(100)).unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (100, 50),
            "resize must fit inside the box and keep the aspect ratio"
        );
        assert_eq!(frame.pixels.len(), 100 * 50 * 4);

        // The opposite skew must round the other way: a 1:2 source in a square
        // box has to come back 50x100.
        let tall = test_png_bytes(100, 200);
        let frame = decode_to_rgba(&tall, Some(100), Some(100)).unwrap();
        assert_eq!((frame.width, frame.height), (50, 100));
    }

    /// The aspect-preserving fit must never upscale past the source, and a
    /// source that already fits must come back untouched — the pre-existing
    /// `test_decode_to_rgba_downscales_when_orig_exceeds_max` only covered the
    /// exact-ratio case, so a switch to `imageops::resize` could have broken
    /// this and still passed.
    #[test]
    fn a_source_that_already_fits_is_not_resized() {
        let buf = test_png_bytes(64, 48);
        // Generous box: no resize should happen, and the RGBA conversion must
        // still have happened (pixel count 4 bytes per pixel, alpha present).
        let frame = decode_to_rgba(&buf, Some(4000), Some(4000)).unwrap();
        assert_eq!((frame.width, frame.height), (64, 48));
        assert_eq!(frame.pixels.len(), 64 * 48 * 4);
    }
}
