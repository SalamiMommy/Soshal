//! Meta PDQ 256-bit Perceptual Image Hashing
//!
//! Implements Meta's ThreatExchange PDQ algorithm for visual similarity matching
//! and zero-tolerance CSAM / violent media deduplication:
//! 1. Grayscale luma downsampling to 64x64.
//! 2. 2D Discrete Cosine Transform (DCT-II).
//! 3. 16x16 frequency coefficient extraction (256 bits).
//! 4. Median thresholding binarization into a 256-bit perceptual hash (32 bytes).
//! 5. Bitwise Hamming distance matching (threshold <= 31 bit diffs).

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::sync::OnceLock;

/// Number of bits in a PDQ hash.
pub const PDQ_HASH_BITS: usize = 256;
/// Default Hamming distance threshold for perceptual visual matching (Meta ThreatExchange standard).
pub const PDQ_MATCH_THRESHOLD: u32 = 31;

/// Result of computing and evaluating a PDQ perceptual hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PdqHashResult {
    /// 64-character hexadecimal representation of the 256-bit hash.
    pub hash_hex: String,
    /// Image visual quality/complexity estimate (0 to 100).
    pub quality: u32,
    /// Whether the hash matches any known threat blocklist entry.
    pub is_threat_match: bool,
    /// Hazard category if matched on a blocklist (e.g. "csam", "gore").
    pub matched_category: Option<String>,
    /// Minimum Hamming distance to a matching blocklist entry.
    pub min_hamming_distance: Option<u32>,
}

impl PdqHashResult {
    pub fn clean(hash_hex: String, quality: u32) -> Self {
        Self {
            hash_hex,
            quality,
            is_threat_match: false,
            matched_category: None,
            min_hamming_distance: None,
        }
    }
}

/// Compute the 256-bit PDQ perceptual hash and quality score from raw image bytes.
pub fn compute_pdq_hash(image_bytes: &[u8]) -> Option<([u8; 32], u32)> {
    if image_bytes.is_empty() {
        return None;
    }

    // Attempt decoding using the image crate
    if let Ok(dyn_img) = image::load_from_memory(image_bytes) {
        let gray = dyn_img.to_luma8();
        let (w, h) = gray.dimensions();
        let luma_matrix = downsample_to_64x64(&gray.into_raw(), w as usize, h as usize);
        return Some(pdq_from_64x64_luma(&luma_matrix));
    }

    // Fallback: if raw RGB or grayscale buffer is provided
    let len = image_bytes.len();
    if len >= 64 * 64 * 3 {
        // Assume RGB
        let dim = (len / 3) as f32;
        let side = dim.sqrt() as usize;
        if side >= 64 {
            let luma_raw: Vec<u8> = image_bytes
                .chunks_exact(3)
                .map(|rgb| {
                    let r = rgb[0] as f32;
                    let g = rgb[1] as f32;
                    let b = rgb[2] as f32;
                    (0.299 * r + 0.587 * g + 0.114 * b) as u8
                })
                .collect();
            let luma_matrix = downsample_to_64x64(&luma_raw, side, side);
            return Some(pdq_from_64x64_luma(&luma_matrix));
        }
    } else if len >= 64 * 64 {
        // Assume raw grayscale
        let side = (len as f32).sqrt() as usize;
        let luma_matrix = downsample_to_64x64(image_bytes, side, side);
        return Some(pdq_from_64x64_luma(&luma_matrix));
    }

    None
}

/// Compute the 256-bit PDQ hash directly from a 64x64 grayscale float matrix.
pub fn pdq_from_64x64_luma(luma: &[f32; 64 * 64]) -> ([u8; 32], u32) {
    // 1. Calculate image quality based on gradient variance
    let quality = calculate_luma_quality(luma);

    // 2. Perform 2D DCT-II to obtain 64x64 frequency coefficients
    let dct = compute_2d_dct_64x64(luma);

    // 3. Extract 16x16 low/mid frequency sub-matrix (256 coefficients)
    let mut coeffs = [0.0f32; 256];
    for u in 0..16 {
        for v in 0..16 {
            // Read 16x16 from low/mid frequencies (offset by 1 to skip pure DC at [0,0])
            let dct_val = dct[(u + 1) * 64 + (v + 1)];
            coeffs[u * 16 + v] = dct_val;
        }
    }

    // 4. Compute median of the 256 coefficients
    let median = compute_median(&coeffs);

    // 5. Binarize into 256-bit hash (32 bytes)
    let mut hash = [0u8; 32];
    for (i, &coeff) in coeffs.iter().enumerate() {
        if coeff > median {
            let byte_idx = i / 8;
            let bit_idx = 7 - (i % 8);
            hash[byte_idx] |= 1 << bit_idx;
        }
    }

    (hash, quality)
}

/// Compute fast separable 2D Discrete Cosine Transform (DCT-II) on 64x64 float buffer.
fn compute_2d_dct_64x64(input: &[f32; 64 * 64]) -> [f32; 64 * 64] {
    let mut temp = [0.0f32; 64 * 64];
    let mut output = [0.0f32; 64 * 64];

    // 1D DCT on rows
    for y in 0..64 {
        for u in 0..64 {
            let mut sum = 0.0f32;
            for x in 0..64 {
                let angle = ((2 * x + 1) as f32 * u as f32 * PI) / 128.0;
                sum += input[y * 64 + x] * angle.cos();
            }
            let alpha = if u == 0 {
                (1.0 / 64.0f32).sqrt()
            } else {
                (2.0 / 64.0f32).sqrt()
            };
            temp[y * 64 + u] = alpha * sum;
        }
    }

    // 1D DCT on columns
    for u in 0..64 {
        for v in 0..64 {
            let mut sum = 0.0f32;
            for y in 0..64 {
                let angle = ((2 * y + 1) as f32 * v as f32 * PI) / 128.0;
                sum += temp[y * 64 + u] * angle.cos();
            }
            let alpha = if v == 0 {
                (1.0 / 64.0f32).sqrt()
            } else {
                (2.0 / 64.0f32).sqrt()
            };
            output[v * 64 + u] = alpha * sum;
        }
    }

    output
}

/// Downsamples an arbitrary sized grayscale buffer into a 64x64 float buffer.
fn downsample_to_64x64(src: &[u8], width: usize, height: usize) -> [f32; 64 * 64] {
    let mut out = [0.0f32; 64 * 64];
    if width == 0 || height == 0 || src.len() < width * height {
        return out;
    }

    let x_scale = width as f32 / 64.0;
    let y_scale = height as f32 / 64.0;

    for dy in 0..64 {
        for dx in 0..64 {
            let sx = (dx as f32 * x_scale) as usize;
            let sy = (dy as f32 * y_scale) as usize;
            let src_idx = (sy.min(height - 1)) * width + (sx.min(width - 1));
            out[dy * 64 + dx] = src[src_idx] as f32;
        }
    }

    out
}

/// Calculate image quality from gradient sharpness and variance (0 to 100).
fn calculate_luma_quality(luma: &[f32; 64 * 64]) -> u32 {
    let mut sum_grad = 0.0f32;
    for y in 0..63 {
        for x in 0..63 {
            let dx = (luma[y * 64 + (x + 1)] - luma[y * 64 + x]).abs();
            let dy = (luma[(y + 1) * 64 + x] - luma[y * 64 + x]).abs();
            sum_grad += dx + dy;
        }
    }
    let avg_grad = sum_grad / (63.0 * 63.0 * 2.0);
    // Scale typical average gradient (0 to 25) to a 0-100 quality metric
    let quality = (avg_grad * 4.0).clamp(0.0, 100.0) as u32;
    quality.max(1)
}

/// Compute median of an array of 256 floats.
fn compute_median(values: &[f32; 256]) -> f32 {
    let mut sorted = *values;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (sorted[127] + sorted[128]) / 2.0
}

/// Compute the bitwise Hamming distance between two 256-bit PDQ hashes.
pub fn pdq_hamming_distance(h1: &[u8; 32], h2: &[u8; 32]) -> u32 {
    let mut dist = 0u32;
    for i in 0..4 {
        let a = u64::from_le_bytes(h1[i * 8..(i + 1) * 8].try_into().unwrap());
        let b = u64::from_le_bytes(h2[i * 8..(i + 1) * 8].try_into().unwrap());
        dist += (a ^ b).count_ones();
    }
    dist
}

/// Known PDQ Blocklist Entry.
#[derive(Debug, Clone)]
pub struct PdqBlocklistEntry {
    pub hash: [u8; 32],
    pub category: String,
    pub description: String,
}

/// Global PDQ Threat Blocklist registry.
pub struct PdqBlocklist {
    entries: Vec<PdqBlocklistEntry>,
}

impl Default for PdqBlocklist {
    fn default() -> Self {
        Self::new()
    }
}

impl PdqBlocklist {
    pub fn new() -> Self {
        let mut entries = Vec::new();

        // Synthetic Sentinel Test Hashes for CSAM and Graphic Violence
        // Sentinel CSAM PDQ Hash
        let csam_hex = "f0f0f0f0f0f0f0f0a5a5a5a5a5a5a5a5123456789abcdef0123456789abcdef0";
        if let Ok(bytes) = hex::decode(csam_hex) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                entries.push(PdqBlocklistEntry {
                    hash: arr,
                    category: "csam".to_string(),
                    description: "Synthetic CSAM Sentinel Hash".to_string(),
                });
            }
        }

        // Sentinel Gore PDQ Hash
        let gore_hex = "0123456789abcdef0123456789abcdeff0f0f0f0f0f0f0f0a5a5a5a5a5a5a5a5";
        if let Ok(bytes) = hex::decode(gore_hex) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                entries.push(PdqBlocklistEntry {
                    hash: arr,
                    category: "gore".to_string(),
                    description: "Synthetic Gore Sentinel Hash".to_string(),
                });
            }
        }

        Self { entries }
    }

    /// Match a target PDQ hash against the blocklist with standard Hamming threshold.
    pub fn check_match(
        &self,
        hash: &[u8; 32],
        max_distance: u32,
    ) -> (bool, Option<String>, Option<u32>) {
        let mut min_dist = u32::MAX;
        let mut matched_cat = None;

        for entry in &self.entries {
            let d = pdq_hamming_distance(hash, &entry.hash);
            if d < min_dist {
                min_dist = d;
                if d <= max_distance {
                    matched_cat = Some(entry.category.clone());
                }
            }
        }

        if min_dist <= max_distance {
            (true, matched_cat, Some(min_dist))
        } else {
            (
                false,
                None,
                if min_dist != u32::MAX {
                    Some(min_dist)
                } else {
                    None
                },
            )
        }
    }
}

static PDQ_BLOCKLIST: OnceLock<PdqBlocklist> = OnceLock::new();

pub fn get_pdq_blocklist() -> &'static PdqBlocklist {
    PDQ_BLOCKLIST.get_or_init(PdqBlocklist::new)
}

/// Evaluate raw media bytes against the PDQ perceptual hashing pipeline.
pub fn evaluate_media_pdq(image_bytes: &[u8]) -> Option<PdqHashResult> {
    let (hash, quality) = compute_pdq_hash(image_bytes)?;
    let hash_hex = hex::encode(hash);
    let (is_match, cat, dist) = get_pdq_blocklist().check_match(&hash, PDQ_MATCH_THRESHOLD);

    Some(PdqHashResult {
        hash_hex,
        quality,
        is_threat_match: is_match,
        matched_category: cat,
        min_hamming_distance: dist,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdq_hamming_distance_exact_and_diff() {
        let h1 = [0xAAu8; 32];
        let h2 = [0xAAu8; 32];
        assert_eq!(pdq_hamming_distance(&h1, &h2), 0);

        let mut h3 = h1;
        h3[0] ^= 0x01; // 1 bit flip
        h3[1] ^= 0x03; // 2 bit flips
        assert_eq!(pdq_hamming_distance(&h1, &h3), 3);
    }

    #[test]
    fn test_pdq_hash_from_luma_matrix() {
        let mut luma = [128.0f32; 64 * 64];
        // Introduce rich 2D frequency variation with cross-terms
        for y in 0..64 {
            for x in 0..64 {
                let fx = x as f32;
                let fy = y as f32;
                luma[y * 64 + x] =
                    128.0 + 40.0 * (fx / 5.0 + fy / 6.0).sin() + 30.0 * ((fx * fy) / 80.0).cos();
            }
        }

        let (hash1, q1) = pdq_from_64x64_luma(&luma);
        assert!(q1 > 0);

        // Slightly modified brightness should retain very low Hamming distance (<= 10)
        let mut luma2 = luma;
        for val in luma2.iter_mut() {
            *val = (*val + 10.0).min(255.0);
        }
        let (hash2, _) = pdq_from_64x64_luma(&luma2);
        let dist = pdq_hamming_distance(&hash1, &hash2);
        assert!(
            dist <= 10,
            "PDQ distance across brightness variation was {}",
            dist
        );
    }

    #[test]
    fn test_pdq_blocklist_matching() {
        let blocklist = PdqBlocklist::new();
        let sentinel_csam_hex = "f0f0f0f0f0f0f0f0a5a5a5a5a5a5a5a5123456789abcdef0123456789abcdef0";
        let mut target = [0u8; 32];
        target.copy_from_slice(&hex::decode(sentinel_csam_hex).unwrap());

        // Exact match
        let (matched, cat, dist) = blocklist.check_match(&target, PDQ_MATCH_THRESHOLD);
        assert!(matched);
        assert_eq!(cat.as_deref(), Some("csam"));
        assert_eq!(dist, Some(0));

        // Perturbed match (5 bit flips <= 31 threshold)
        target[0] ^= 0x1F;
        let (matched_p, cat_p, dist_p) = blocklist.check_match(&target, PDQ_MATCH_THRESHOLD);
        assert!(matched_p);
        assert_eq!(cat_p.as_deref(), Some("csam"));
        assert_eq!(dist_p, Some(5));

        // Unrelated hash (completely different)
        let random_hash = [0x00u8; 32];
        let (matched_r, _, _) = blocklist.check_match(&random_hash, PDQ_MATCH_THRESHOLD);
        assert!(!matched_r);
    }
}
