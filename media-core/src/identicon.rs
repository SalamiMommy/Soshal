//! Deterministic identicon avatars derived from a string seed (pubkey).
//!
//! Renders a 5x5 mirrored grid (classic identicon style) as a PNG using only
//! std FNV-1a hashing and the `image` crate — no randomness, no extra deps.

use std::collections::VecDeque;
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

const CACHE_CAP: usize = 128;

type AvatarCache = VecDeque<(String, Vec<u8>)>;

static AVATAR_CACHE: OnceLock<Mutex<AvatarCache>> = OnceLock::new();

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
    let cache = AVATAR_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    {
        let mut guard = cache.lock().unwrap();
        if let Some(pos) = guard.iter().position(|(k, _)| k == seed) {
            let entry = guard.remove(pos).expect("position from iter");
            let png = entry.1.clone();
            guard.push_back(entry);
            return png;
        }
    }
    let hash = fnv1a(seed);
    let (fg, bg) = colors(hash);
    let mut img = image::RgbaImage::new(SIZE, SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (cx, cy) = ((x / CELL) as usize, (y / CELL) as usize);
            // Mirror: derive the right 2 columns from the left 3.
            let cx_m = cx.min(GRID - 1 - cx);
            let bit = (cx_m * GRID + cy) % 64;
            let filled = (hash >> bit) & 1 == 1;
            let in_cell = x % CELL >= BORDER
                && x % CELL < CELL - BORDER
                && y % CELL >= BORDER
                && y % CELL < CELL - BORDER;
            let color = if in_cell && filled { fg } else { bg };
            img.put_pixel(x, y, image::Rgba([color[0], color[1], color[2], 255]));
        }
    }
    let mut cursor = Cursor::new(Vec::new());
    let png =
        match image::DynamicImage::ImageRgba8(img).write_to(&mut cursor, image::ImageFormat::Png) {
            Ok(()) => cursor.into_inner(),
            Err(_) => FALLBACK_PNG.to_vec(),
        };
    {
        let mut guard = cache.lock().unwrap();
        if guard.len() >= CACHE_CAP {
            guard.pop_front();
        }
        guard.push_back((seed.to_string(), png.clone()));
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
