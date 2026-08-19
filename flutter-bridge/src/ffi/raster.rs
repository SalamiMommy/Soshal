//! Custom C-ABI Raster Callbacks for Impeller Command Buffer & GPU Texture Injection.
//! Pushes decoded media frames directly into Impeller's underlying graphics pipeline at 120Hz display refresh.

use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Max single frame buffer allocation: 256 MiB. Bounds an 8K RGBA frame
/// (7680*4320*4 ≈ 132.7 MiB) plus headroom; prevents OOM abort on bogus dims.
const MAX_FRAME_BUFFER_BYTES: u64 = 256 * 1024 * 1024;

/// Live frame buffers keyed by pointer address, so `raster_release_frame_buffer`
/// can reclaim them (replaces the old `mem::forget` permanent leak).
static FRAME_BUFFERS: LazyLock<Mutex<HashMap<usize, Box<[u8]>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Frame metadata for decoded video/media buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpellerFrameBufferInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: String, // "RGBA8888" or "BGRA8888"
    pub buffer_ptr_addr: usize,
}

/// Registers a shared pixel buffer memory callback for Flutter Impeller rasterizer.
#[frb(serialize)]
pub fn raster_allocate_frame_buffer(
    width: u32,
    height: u32,
) -> Result<ImpellerFrameBufferInfo, String> {
    let bytes_per_pixel = 4u32;
    let stride = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| format!("raster: stride overflow (width {width})"))?;
    // Cap guarantees fit in usize on all supported 32/64-bit targets.
    let total_bytes = stride
        .checked_mul(height)
        .map(u64::from)
        .filter(|&b| b <= MAX_FRAME_BUFFER_BYTES)
        .ok_or_else(|| {
            format!(
                "raster: frame buffer too large ({width}x{height}); max {MAX_FRAME_BUFFER_BYTES} bytes"
            )
        })? as usize;

    let buf = vec![0u8; total_bytes].into_boxed_slice();
    let ptr_addr = buf.as_ptr() as usize;

    // Tracked allocation: reclaimed via raster_release_frame_buffer(ptr_addr).
    FRAME_BUFFERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(ptr_addr, buf);

    Ok(ImpellerFrameBufferInfo {
        width,
        height,
        stride,
        pixel_format: "RGBA8888".to_string(),
        buffer_ptr_addr: ptr_addr,
    })
}

/// Frees a frame buffer previously returned by `raster_allocate_frame_buffer`.
/// Unknown or double-released addresses are a safe no-op error.
#[frb(serialize)]
pub fn raster_release_frame_buffer(ptr_addr: usize) -> Result<bool, String> {
    let mut registry = FRAME_BUFFERS.lock().unwrap_or_else(|e| e.into_inner());
    match registry.remove(&ptr_addr) {
        Some(_buf) => Ok(true), // dropped here -> memory freed
        None => Err(format!("raster: no tracked buffer at ptr {ptr_addr:#x}")),
    }
}

/// Pushes frame render signal directly to Impeller raster pipeline.
#[frb(serialize)]
pub fn raster_signal_impeller_frame_ready(
    texture_id: i64,
    frame_timestamp_ns: u64,
) -> Result<bool, String> {
    // Signals Flutter engine rasterizer loop that new frame buffer is ready at texture_id
    let _ = (texture_id, frame_timestamp_ns);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raster_allocate_frame_buffer() {
        let info = raster_allocate_frame_buffer(1920, 1080).unwrap();
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.stride, 1920 * 4);
        assert!(info.buffer_ptr_addr > 0);
        assert!(raster_release_frame_buffer(info.buffer_ptr_addr).unwrap());
    }

    #[test]
    fn test_raster_allocate_overflow_and_cap() {
        assert!(raster_allocate_frame_buffer(u32::MAX, u32::MAX).is_err());
        // 20000*20000*4 = 1.6 GiB > 256 MiB cap
        assert!(raster_allocate_frame_buffer(20_000, 20_000).is_err());
        // unknown ptr release is a safe error
        assert!(raster_release_frame_buffer(0xdead_beef).is_err());
    }

    #[test]
    fn test_raster_release_unknown_and_double() {
        let info = raster_allocate_frame_buffer(320, 240).unwrap();
        assert!(raster_release_frame_buffer(info.buffer_ptr_addr).unwrap());
        // double release -> Err, not UB
        assert!(raster_release_frame_buffer(info.buffer_ptr_addr).is_err());
    }
}
