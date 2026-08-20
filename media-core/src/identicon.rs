//! Deterministic identicon avatars derived from a string seed (pubkey).
//!
//! Renders a 5x5 mirrored grid (classic identicon style) as a PNG using only
//! std FNV-1a hashing and the `image` crate — no randomness, no extra deps.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::{Mutex, OnceLock};

/// Minimal 1x1 transparent PNG, returned if PNG encoding ever fails.
const FALLBACK_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// Grid cells per side.
const GRID: usize = 5;
/// Output image edge in pixels.
const SIZE: u32 = 200;
/// Pixels per grid cell (SIZE / GRID).
const CELL: u32 = 40;
/// Border thickness inside each cell, in pixels.
const BORDER: u32 = 4;

const CACHE_CAP: usize = 256;

struct LruAvatarCache {
    map: HashMap<String, Vec<u8>>,
    order: VecDeque<String>,
}

static AVATAR_CACHE: OnceLock<Mutex<LruAvatarCache>> = OnceLock::new();

/// FNV-1a 64-bit hash over seed bytes (std only).
fn fnv1a(seed: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Foreground/background colors derived from the hash bytes.
fn colors(hash: u64) -> ([u8; 3], [u8; 3]) {
    let b = hash.to_le_bytes();
    let fg = [b[0].max(0x30), b[1].max(0x30), b[2].max(0x30)];
    let bg = [0xE0 | (b[3] >> 4), 0xE0 | (b[4] >> 4), 0xE0 | (b[5] >> 4)];
    (fg, bg)
}

/// Deterministic avatar PNG bytes for a seed (pubkey). Same seed → same bytes.
pub fn identicon_png(seed: &str) -> Vec<u8> {
    let cache = AVATAR_CACHE.get_or_init(|| {
        Mutex::new(LruAvatarCache {
            map: HashMap::new(),
            order: VecDeque::new(),
        })
    });
    {
        let guard = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(png) = guard.map.get(seed) {
            return png.clone();
        }
    }
    let hash = fnv1a(seed);
    let (fg, bg) = colors(hash);

    // Precompute 5x5 cell grid fill state once
    let mut grid_filled = [[false; GRID]; GRID];
    for (cx, row) in grid_filled.iter_mut().enumerate() {
        let cx_m = cx.min(GRID - 1 - cx);
        for (cy, cell) in row.iter_mut().enumerate() {
            let bit = (cx_m * GRID + cy) % 64;
            *cell = (hash >> bit) & 1 == 1;
        }
    }

    let fg_rgba = [fg[0], fg[1], fg[2], 255];
    let bg_rgba = [bg[0], bg[1], bg[2], 255];

    // Precompute coordinate and border lookup tables once for SIZE (200) elements.
    let mut col_cx = [0usize; SIZE as usize];
    let mut col_in_cell = [false; SIZE as usize];
    for (x, (cx_slot, in_cell_slot)) in col_cx.iter_mut().zip(col_in_cell.iter_mut()).enumerate() {
        let x_u32 = x as u32;
        *cx_slot = (x_u32 / CELL) as usize;
        let rem = x_u32 % CELL;
        *in_cell_slot = (BORDER..CELL - BORDER).contains(&rem);
    }

    let mut raw_pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        let cy = (y / CELL) as usize;
        let rem_y = y % CELL;
        let in_cell_y = (BORDER..CELL - BORDER).contains(&rem_y);
        let row_offset = (y * SIZE * 4) as usize;

        for x in 0..SIZE as usize {
            let in_cell_x = col_in_cell[x];
            let cx = col_cx[x];
            let px_offset = row_offset + (x * 4);

            let color = if in_cell_x && in_cell_y && grid_filled[cx][cy] {
                fg_rgba
            } else {
                bg_rgba
            };

            raw_pixels[px_offset..px_offset + 4].copy_from_slice(&color);
        }
    }

    let img = image::RgbaImage::from_raw(SIZE, SIZE, raw_pixels)
        .unwrap_or_else(|| image::RgbaImage::new(SIZE, SIZE));

    let mut cursor = Cursor::new(Vec::new());
    let png =
        match image::DynamicImage::ImageRgba8(img).write_to(&mut cursor, image::ImageFormat::Png) {
            Ok(()) => cursor.into_inner(),
            Err(_) => FALLBACK_PNG.to_vec(),
        };
    {
        let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());
        if !guard.map.contains_key(seed) {
            if guard.order.len() >= CACHE_CAP {
                if let Some(oldest) = guard.order.pop_front() {
                    guard.map.remove(&oldest);
                }
            }
            guard.order.push_back(seed.to_string());
            guard.map.insert(seed.to_string(), png.clone());
        }
    }
    png
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_same_seed() {
        let a = identicon_png("npub1examplepubkey");
        let b = identicon_png("npub1examplepubkey");
        assert_eq!(a, b);
        assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(
            identicon_png("npub1examplepubkey"),
            identicon_png("npub1examplepubkez")
        );
    }

    #[test]
    fn non_empty_output() {
        assert!(!identicon_png("").is_empty());
        assert!(!identicon_png("npub1abc").is_empty());
    }
}
