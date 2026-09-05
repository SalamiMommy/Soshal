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

/// Drain the decoder: YUV_420_888 → I420 → RGB → JPEG.
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
        let mut image: *mut AImage = std::ptr::null_mut();
        if AMediaCodec_getOutputImage(codec, idx as usize, &mut image) == 0 && !image.is_null() {
            if let Ok(jpeg) = image_to_jpeg(image) {
                out.push(jpeg);
            }
            AImage_release(image);
        }
        AMediaCodec_releaseOutputBuffer(codec, idx as usize, 0);
        if out.len() >= 8 {
            break;
        }
    }
    out
}

/// AImage (YUV_420_888, planar with strides) → JPEG q60.
#[cfg(target_os = "android")]
unsafe fn image_to_jpeg(image: *mut AImage) -> Result<Vec<u8>, String> {
    let mut w = 0i32;
    let mut h = 0i32;
    if AImage_getWidth(image, &mut w) != 0 || AImage_getHeight(image, &mut h) != 0 {
        return Err("image dims".to_string());
    }
    if w % 2 != 0 || h % 2 != 0 || w <= 0 || h <= 0 {
        return Err("bad dims".to_string());
    }
    const MAX_DIM: i32 = 7680;
    const MAX_PIXELS: i64 = 33_177_600;
    if w > MAX_DIM || h > MAX_DIM || (w as i64) * (h as i64) > MAX_PIXELS {
        return Err("dims too large".to_string());
    }
    let mut planes = 0i32;
    if AImage_getNumberOfPlanes(image, &mut planes) != 0 || planes < 3 {
        return Err("no planes".to_string());
    }
    let mut y = std::ptr::null_mut();
    let mut y_len = 0i32;
    let mut y_stride = 0i32;
    let mut y_ps = 0i32;
    AImage_getPlaneData(image, 0, &mut y, &mut y_len);
    AImage_getPlaneRowStride(image, 0, &mut y_stride);
    AImage_getPlanePixelStride(image, 0, &mut y_ps);
    let mut u = std::ptr::null_mut();
    let mut u_len = 0i32;
    let mut u_stride = 0i32;
    let mut u_ps = 0i32;
    AImage_getPlaneData(image, 1, &mut u, &mut u_len);
    AImage_getPlaneRowStride(image, 1, &mut u_stride);
    AImage_getPlanePixelStride(image, 1, &mut u_ps);
    let mut v = std::ptr::null_mut();
    let mut v_len = 0i32;
    let mut v_stride = 0i32;
    let mut v_ps = 0i32;
    AImage_getPlaneData(image, 2, &mut v, &mut v_len);
    AImage_getPlaneRowStride(image, 2, &mut v_stride);
    AImage_getPlanePixelStride(image, 2, &mut v_ps);
    if y.is_null()
        || u.is_null()
        || v.is_null()
        || y_ps <= 0
        || u_ps <= 0
        || v_ps <= 0
        || y_stride <= 0
        || u_stride <= 0
        || v_stride <= 0
    {
        return Err("plane data".to_string());
    }
    if y_len <= 0 || u_len <= 0 || v_len <= 0 {
        return Err("plane lengths invalid".to_string());
    }

    let mut rgb = image::RgbImage::new(w as u32, h as u32);
    let (y_b, u_b, v_b) = (
        std::slice::from_raw_parts(y, y_len as usize),
        std::slice::from_raw_parts(u, u_len as usize),
        std::slice::from_raw_parts(v, v_len as usize),
    );
    for row in 0..h {
        for col in 0..w {
            let y_idx = (row as i64) * (y_stride as i64) + (col as i64) * (y_ps as i64);
            let yv = *y_b.get(y_idx as usize).unwrap_or(&16) as i32;
            let uv_row = row / 2;
            let uv_col = col / 2;
            let uv_idx = (uv_row as i64) * (u_stride as i64) + (uv_col as i64) * (u_ps as i64);
            let uv = *u_b.get(uv_idx as usize).unwrap_or(&128) as i32;
            let vv = *v_b
                .get((uv_row as i64 * (v_stride as i64) + (uv_col as i64) * (v_ps as i64)) as usize)
                .unwrap_or(&128) as i32;
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
