//! H.264 hardware encode/decode via NDK AMediaCodec (Android only).
//!
//! Encode: BGRA frames (camera plugin bgra8888) → I420 → AVC encoder;
//! drained Annex-B NAL blobs returned as `[flag, ...nal]` (flag 1 = key
//! frame), mirrored to the Kotlin LiveRecorder DVR via JNI.
//! Decode: Annex-B NAL blobs → software AVC decoder → YUV_420_888 AImage →
//! I420 → RGB → JPEG (pure-Rust `image` crate, no Java YuvImage).

#![allow(unsafe_code)]

#[cfg(target_os = "android")]
use super::ndk::*;
use super::video_state;

/// True only on Android ≥ 26 with a hardware AVC encoder present.
pub fn is_supported() -> bool {
    #[cfg(target_os = "android")]
    {
        if !super::sdk_gate() {
            return false;
        }
        unsafe {
            let codec = AMediaCodec_createEncoderByType(c"video/avc".as_ptr());
            if codec.is_null() {
                return false;
            }
            AMediaCodec_delete(codec);
        }
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Configure the hardware AVC encoder. Returns false when unavailable.
pub fn init_encode(width: i32, height: i32, bitrate: i32, fps: i32) -> bool {
    #[cfg(target_os = "android")]
    {
        const MAX_DIM: i32 = 7680;
        const MAX_PIXELS: i64 = 33_177_600;
        if !super::sdk_gate()
            || width <= 0
            || height <= 0
            || width % 2 != 0
            || height % 2 != 0
            || width > MAX_DIM
            || height > MAX_DIM
            || (width as i64 * height as i64) > MAX_PIXELS
        {
            return false;
        }
        let mut s = video_state();
        s.release_h264_encoder_only();
        unsafe {
            let codec = AMediaCodec_createEncoderByType(c"video/avc".as_ptr());
            if codec.is_null() {
                return false;
            }
            let fmt = AMediaFormat_new();
            if fmt.is_null() {
                AMediaCodec_delete(codec);
                return false;
            }
            AMediaFormat_setString(fmt, c"mime".as_ptr(), c"video/avc".as_ptr());
            AMediaFormat_setInt32(fmt, c"width".as_ptr(), width);
            AMediaFormat_setInt32(fmt, c"height".as_ptr(), height);
            AMediaFormat_setInt32(fmt, c"color-format".as_ptr(), COLOR_FormatYUV420Flexible);
            AMediaFormat_setInt32(fmt, c"bitrate".as_ptr(), bitrate);
            AMediaFormat_setInt32(fmt, c"frame-rate".as_ptr(), fps);
            AMediaFormat_setInt32(fmt, c"i-frame-interval".as_ptr(), 1);
            let ok = AMediaCodec_configure(
                codec,
                fmt,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                AMEDIACODEC_CONFIGURE_FLAG_ENCODE,
            ) == 0
                && AMediaCodec_start(codec) == 0;
            AMediaFormat_delete(fmt);
            if !ok {
                AMediaCodec_delete(codec);
                return false;
            }
            s.h264_encoder = Some(super::NativeCodec(codec));
            s.h264_width = width;
            s.h264_height = height;
        }
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (width, height, bitrate, fps);
        false
    }
}

/// Feed one BGRA frame; returns drained `[flag, ...Annex-B]` blobs.
pub fn feed_encode(bgra: &[u8]) -> Vec<Vec<u8>> {
    #[cfg(target_os = "android")]
    {
        let s = video_state();
        let Some(codec) = s.h264_encoder.as_ref() else {
            return Vec::new();
        };
        let codec = codec.0;
        let (w, h) = (s.h264_width, s.h264_height);
        if w <= 0 || h <= 0 {
            return Vec::new();
        }
        let need = match (w as usize)
            .checked_mul(h as usize)
            .and_then(|p| p.checked_mul(4))
        {
            Some(n) => n,
            None => return Vec::new(),
        };
        if bgra.len() < need {
            return Vec::new();
        }
        unsafe {
            let (w_u, h_u) = (w as usize, h as usize);
            let needed_i420 = w_u * h_u + ((w_u + 1) / 2) * ((h_u + 1) / 2) * 2;
            let idx = AMediaCodec_dequeueInputBuffer(codec, 0);
            if idx >= 0 {
                let mut size = 0usize;
                let buf = AMediaCodec_getInputBuffer(codec, idx as usize, &mut size);
                if !buf.is_null() && size >= needed_i420 {
                    let dst = std::slice::from_raw_parts_mut(buf, needed_i420);
                    bgra_to_i420_into(bgra, w, h, dst);
                    AMediaCodec_queueInputBuffer(codec, idx as usize, 0, needed_i420, 0, 0);
                } else {
                    AMediaCodec_queueInputBuffer(codec, idx as usize, 0, 0, 0, 0);
                }
            }
            drain_encoder(codec, w, h)
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = bgra;
        Vec::new()
    }
}

/// Configure the software AVC decoder.
pub fn init_decode() -> bool {
    #[cfg(target_os = "android")]
    {
        if !super::sdk_gate() {
            return false;
        }
        let mut s = video_state();
        s.release_h264_decoder_only();
        unsafe {
            let codec = AMediaCodec_createDecoderByType(c"video/avc".as_ptr());
            if codec.is_null() {
                return false;
            }
            let fmt = AMediaFormat_new();
            if fmt.is_null() {
                AMediaCodec_delete(codec);
                return false;
            }
            AMediaFormat_setString(fmt, c"mime".as_ptr(), c"video/avc".as_ptr());
            let ok =
                AMediaCodec_configure(codec, fmt, std::ptr::null_mut(), std::ptr::null_mut(), 0)
                    == 0
                    && AMediaCodec_start(codec) == 0;
            AMediaFormat_delete(fmt);
            if !ok {
                AMediaCodec_delete(codec);
                return false;
            }
            s.h264_decoder = Some(super::NativeCodec(codec));
        }
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Feed one Annex-B NAL blob; returns JPEG frames drained from the decoder.
pub fn feed_decode(nal: &[u8]) -> Vec<Vec<u8>> {
    #[cfg(target_os = "android")]
    {
        if nal.is_empty() {
            return Vec::new();
        }
        let s = video_state();
        let Some(codec) = s.h264_decoder.as_ref() else {
            return Vec::new();
        };
        let codec = codec.0;
        unsafe {
            let idx = AMediaCodec_dequeueInputBuffer(codec, 0);
            if idx >= 0 {
                let mut size = 0usize;
                let buf = AMediaCodec_getInputBuffer(codec, idx as usize, &mut size);
                if !buf.is_null() && size >= nal.len() {
                    std::ptr::copy_nonoverlapping(nal.as_ptr(), buf, nal.len());
                    AMediaCodec_queueInputBuffer(codec, idx as usize, 0, nal.len(), 0, 0);
                } else {
                    AMediaCodec_queueInputBuffer(codec, idx as usize, 0, 0, 0, 0);
                }
            }
            drain_decoder(codec)
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = nal;
        Vec::new()
    }
}

/// Release encoder + decoder.
pub fn release() -> bool {
    let mut s = video_state();
    s.release_h264();
    true
}

/// Drain the encoder: tagged NAL blobs, mirrored to the DVR muxer.
#[cfg(target_os = "android")]
unsafe fn drain_encoder(codec: *mut AMediaCodec, width: i32, height: i32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut info = AMediaCodecBufferInfo {
        offset: 0,
        size: 0,
        presentation_time_us: 0,
        flags: 0,
    };
    loop {
        let idx = AMediaCodec_dequeueOutputBuffer(codec, &mut info, 0);
        if idx == AMEDIACODEC_INFO_TRY_AGAIN_LATER {
            break;
        }
        if idx == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED
            || idx == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
        {
            continue;
        }
        if idx < 0 {
            break;
        }
        let mut size = 0usize;
        let buf = AMediaCodec_getOutputBuffer(codec, idx as usize, &mut size);
        if !buf.is_null()
            && info.size > 0
            && info.offset >= 0
            && (info.offset as usize).saturating_add(info.size as usize) <= size
        {
            let payload =
                std::slice::from_raw_parts(buf.offset(info.offset as isize), info.size as usize);
            let is_key = info.flags & AMEDIACODEC_BUFFER_FLAG_KEY_FRAME != 0;
            let is_config = info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG != 0;
            let _ = super::dvr_write_video(payload, is_key, is_config, width, height);
            let flag: u8 = if is_key { 1 } else { 0 };
            let mut tagged = Vec::with_capacity(payload.len() + 1);
            tagged.push(flag);
            tagged.extend_from_slice(payload);
            out.push(tagged);
        }
        AMediaCodec_releaseOutputBuffer(codec, idx as usize, 0);
        if out.len() >= 32 {
            break;
        }
    }
    out
}

/// Drain the decoder: raw YUV420 → RGB → JPEG.
#[cfg(target_os = "android")]
unsafe fn drain_decoder(codec: *mut AMediaCodec) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut info = AMediaCodecBufferInfo {
        offset: 0,
        size: 0,
        presentation_time_us: 0,
        flags: 0,
    };
    loop {
        let idx = AMediaCodec_dequeueOutputBuffer(codec, &mut info, 0);
        if idx == AMEDIACODEC_INFO_TRY_AGAIN_LATER {
            break;
        }
        if idx == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED
            || idx == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED
        {
            continue;
        }
        if idx < 0 {
            break;
        }
        let mut size = 0usize;
        let buf = AMediaCodec_getOutputBuffer(codec, idx as usize, &mut size);
        if !buf.is_null() && info.size > 0 && info.offset >= 0 {
            let fmt = AMediaCodec_getOutputFormat(codec);
            let mut w = 0i32;
            let mut h = 0i32;
            let mut color_fmt = 0i32;
            let mut stride = 0i32;
            let mut slice_height = 0i32;
            if !fmt.is_null() {
                AMediaFormat_getInt32(fmt, c"width".as_ptr(), &mut w);
                AMediaFormat_getInt32(fmt, c"height".as_ptr(), &mut h);
                AMediaFormat_getInt32(fmt, c"color-format".as_ptr(), &mut color_fmt);
                AMediaFormat_getInt32(fmt, c"stride".as_ptr(), &mut stride);
                AMediaFormat_getInt32(fmt, c"slice-height".as_ptr(), &mut slice_height);
                AMediaFormat_delete(fmt);
            }
            if stride <= 0 {
                stride = w;
            }
            if slice_height <= 0 {
                slice_height = h;
            }
            if w > 0 && h > 0 && (info.offset as usize).saturating_add(info.size as usize) <= size {
                let payload = std::slice::from_raw_parts(
                    buf.offset(info.offset as isize),
                    info.size as usize,
                );
                let (rw, rh, x_step, y_step) =
                    render_footprint(w as usize, h as usize, MAX_DECODE_OUTPUT_PIXELS);
                if let Ok(jpeg) = yuv420_to_jpeg(
                    payload,
                    w as usize,
                    h as usize,
                    stride as usize,
                    slice_height as usize,
                    color_fmt,
                    rw,
                    rh,
                    x_step,
                    y_step,
                ) {
                    out.push(jpeg);
                }
            }
        }
        AMediaCodec_releaseOutputBuffer(codec, idx as usize, 0);
        if out.len() >= 8 {
            break;
        }
    }
    out
}

/// Cap on the DECODED output footprint for software YUV→JPEG conversion.
/// An 8K hostile frame would otherwise allocate a ~100 MB `RgbImage` (8K²
/// × 3 bytes). Frames larger than this are downsampled by sampling the
/// source plane; 8 MP ≈ 4K at 2x density, plenty for a viewer thumbnail.
#[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
pub const MAX_DECODE_OUTPUT_PIXELS: usize = 8 * 1024 * 1024;

/// Pure policy: given source dims, return `(render_w, render_h, x_step,
/// y_step)` such that `render_w * render_h <= max_pixels`. The render grid
/// downsamples the source YUV plane by taking one pixel every `step` on
/// each axis — no resampler allocation, so hostile (huge, untrusted) decode
/// input cannot force a multi-hundred-MB heap spike in the codec fast path.
#[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
fn render_footprint(w: usize, h: usize, max_pixels: usize) -> (usize, usize, usize, usize) {
    if w == 0 || h == 0 || max_pixels == 0 {
        return (0, 0, 1, 1);
    }
    let mut rw = w;
    let mut rh = h;
    let total = w * h;
    if total > max_pixels {
        let scale = (total as f64 / max_pixels as f64).sqrt();
        rw = ((w as f64 / scale).floor() as usize).max(1);
        rh = ((h as f64 / scale).floor() as usize).max(1);
        // Exact aspect-preserving trim onto the pixel budget.
        while rw * rh > max_pixels && rw > 1 && rh > 1 {
            if rw as f64 / rh as f64 >= w as f64 / h as f64 {
                rw -= 1;
            } else {
                rh -= 1;
            }
        }
    }
    // Integer steps guarantee (step-1 index) * ... stays within the source
    // plane: x_step = w / rw => (rw - 1) * x_step <= w - 1.
    let x_step = (w / rw).max(1);
    let y_step = (h / rh).max(1);
    (rw, rh, x_step, y_step)
}

/// YUV420 buffer (I420, NV12, or NV21) → JPEG q60.
#[cfg(target_os = "android")]
unsafe fn yuv420_to_jpeg(
    yuv: &[u8],
    w: usize,
    h: usize,
    stride: usize,
    slice_h: usize,
    color_format: i32,
    rw: usize,
    rh: usize,
    x_step: usize,
    y_step: usize,
) -> Result<Vec<u8>, String> {
    if w == 0 || h == 0 || yuv.is_empty() || rw == 0 || rh == 0 {
        return Err("empty dims or buffer".to_string());
    }
    const MAX_DIM: usize = 7680;
    const MAX_PIXELS: usize = 33_177_600;
    if w > MAX_DIM || h > MAX_DIM || w * h > MAX_PIXELS {
        return Err("dims too large".to_string());
    }

    let mut rgb = image::RgbImage::new(rw as u32, rh as u32);
    let y_plane_size = stride * slice_h;
    let is_semi_planar = color_format != 19; // 19 is COLOR_FormatYUV420Planar (I420)
    for row in 0..rh {
        let src_row = row * y_step;
        for col in 0..rw {
            let src_col = col * x_step;
            let y_idx = src_row * stride + src_col;
            let yv = *yuv.get(y_idx).unwrap_or(&16) as i32;
            let uv_row = src_row / 2;
            let uv_col = src_col / 2;
            let (uv, vv) = if is_semi_planar {
                let uv_idx = y_plane_size + uv_row * stride + uv_col * 2;
                let u = *yuv.get(uv_idx).unwrap_or(&128) as i32;
                let v = *yuv.get(uv_idx + 1).unwrap_or(&128) as i32;
                if color_format == 21 {
                    (v, u)
                } else {
                    (u, v)
                }
            } else {
                let uv_stride = (stride + 1) / 2;
                let u_idx = y_plane_size + uv_row * uv_stride + uv_col;
                let v_idx = y_plane_size + (y_plane_size / 4) + uv_row * uv_stride + uv_col;
                let u = *yuv.get(u_idx).unwrap_or(&128) as i32;
                let v = *yuv.get(v_idx).unwrap_or(&128) as i32;
                (u, v)
            };
            let c = yv - 16;
            let d = uv - 128;
            let e = vv - 128;
            let r = (298 * c + 409 * e + 128) >> 8;
            let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
            let b = (298 * c + 516 * d + 128) >> 8;
            rgb.put_pixel(
                col as u32,
                row as u32,
                image::Rgb([
                    r.clamp(0, 255) as u8,
                    g.clamp(0, 255) as u8,
                    b.clamp(0, 255) as u8,
                ]),
            );
        }
    }
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 60);
    enc.encode_image(&image::DynamicImage::ImageRgb8(rgb))
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// BT.601 studio-swing BGRA (camera bgra8888) → packed planar I420.
/// Writes directly into destination buffer without intermediate heap allocation.
#[cfg(target_os = "android")]
fn bgra_to_i420_into(bgra: &[u8], width: i32, height: i32, i420: &mut [u8]) {
    let (w, h) = (width as usize, height as usize);
    let (w2, h2) = ((w + 1) / 2, (h + 1) / 2);
    let mut y_pos = 0usize;
    let mut u_pos = w * h;
    let mut v_pos = w * h + w2 * h2;
    let mut p = 0usize;
    for y in 0..h {
        let row_even = y % 2 == 0;
        for x in 0..w {
            let b = bgra[p] as i32;
            let g = bgra[p + 1] as i32;
            let r = bgra[p + 2] as i32;
            p += 4;
            i420[y_pos] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
            y_pos += 1;
            if row_even && x % 2 == 0 && u_pos < w * h + w2 * h2 && v_pos < i420.len() {
                i420[u_pos] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8;
                i420[v_pos] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8;
                u_pos += 1;
                v_pos += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_small_frames_pass_through_unchanged() {
        let (rw, rh, xs, ys) = render_footprint(320, 240, MAX_DECODE_OUTPUT_PIXELS);
        assert_eq!((rw, rh), (320, 240));
        assert_eq!((xs, ys), (1, 1));
    }

    #[test]
    fn footprint_8k_frame_capped_to_8mp() {
        // 8K > 8 MP: must downscale, never exceed the pixel budget.
        let (rw, rh, xs, ys) = render_footprint(7680, 4320, MAX_DECODE_OUTPUT_PIXELS);
        assert!(rw * rh <= MAX_DECODE_OUTPUT_PIXELS, "{rw}*{rh}");
        assert!(rw > 0 && rh > 0);
        // Near-16:9 aspect is preserved.
        assert!((rw as f64 / rh as f64 - 7680.0 / 4320.0).abs() < 0.02);
        // Steps stay in-bounds: last sampled index < source dim.
        assert!((rw - 1) * xs < 7680);
        assert!((rh - 1) * ys < 4320);
    }

    #[test]
    fn footprint_4k_1080p_fits_budget_unchanged() {
        // 3840*2160 = 8.29 MP <= 8.39 MP budget: no downscale needed.
        let (rw, rh, xs, ys) = render_footprint(3840, 2160, MAX_DECODE_OUTPUT_PIXELS);
        assert_eq!((rw, rh), (3840, 2160));
        assert_eq!((xs, ys), (1, 1));
        // Slightly over budget forces downscale.
        let (rw2, rh2, _, _) = render_footprint(4000, 2400, MAX_DECODE_OUTPUT_PIXELS);
        // 4000*2400 = 9.6 MP > 8.39 MP budget.
        assert!(rw2 * rh2 <= MAX_DECODE_OUTPUT_PIXELS);
        assert!(rw2 < 4000 && rh2 < 2400);
    }

    #[test]
    fn footprint_respects_smaller_custom_budgets() {
        let (rw, rh, xs, ys) = render_footprint(1920, 1080, 250_000);
        assert!(rw * rh <= 250_000);
        assert!(rw >= 1 && rh >= 1);
        assert!(xs >= 1 && ys >= 1);
        // Degenerate inputs.
        let (rw2, rh2, _, _) = render_footprint(0, 100, 1000);
        assert_eq!((rw2, rh2), (0, 0));
        let (rw3, rh3, _, _) = render_footprint(100, 0, 1000);
        assert_eq!((rw3, rh3), (0, 0));
    }

    #[test]
    fn footprint_steps_never_sample_out_of_bounds() {
        for (w, h) in [(1, 1), (2, 2), (7, 5), (6, 6), (17, 9), (100, 100), (5, 5)] {
            let (rw, rh, xs, ys) = render_footprint(w, h, 20);
            assert!(rw <= w && rh <= h);
            if rw > 0 {
                assert!((rw - 1) * xs < w, "x last sample OOB for {w}x{h}");
                assert!((rh - 1) * ys < h, "y last sample OOB for {w}x{h}");
            }
        }
    }
}
