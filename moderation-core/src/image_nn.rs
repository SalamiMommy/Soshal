//! Lightweight int8-able CNN for image moderation.
//!
//! Tiny 3-conv network run on a 96x96 RGB downscale, trained offline by
//! `ml/scripts/train_image.py`:
//! ```text
//! conv3x3/2: 3 -> 16 (48x48)  ReLU
//! conv3x3/2: 16 -> 32 (24x24) ReLU
//! conv3x3/2: 32 -> 32 (12x12) ReLU
//! concat(GAP, GMP) -> 64 -> 3 heads: [gore, nudity, juvenile] sigmoid
//! ```
//! ~15k parameters (~64 KiB fp32). Pooling concatenates the global mean and
//! global max of each conv3 channel: the max branch preserves local/texture
//! cues (age, gore details) that plain average pooling averages away — the
//! juvenile head's discrimination measurably improves over GAP-alone
//! (shipped juvenile head: 0.923 recall / 0.411 precision on held-out
//! UTKFace faces at the 0.5 gate, vs 0.66 / 0.23 for GAP pooling). The 3
//! heads compose into moderation signals:
//! - `gore` head flags graphic/bloody imagery (fuses with chrominance).
//! - `nudity x juvenile` composes the CSAM *risk* signal. Adult nudity alone
//!   MUST NOT flag (Soshal policy: nudity is fine, CSAM is not) — both heads
//!   must agree before a risk is emitted, and the risk only feeds the
//!   report/review gate, never silent auto-deletion.
//!
//! Honesty guard: the embedded weight file is absent until legal training
//! data is curated (see ml/README.md). Until then `classify_image_bytes`
//! returns `None` and the existing deterministic chrominance / hash layers
//! stay authoritative — the NN never silently simulates.
#![allow(clippy::needless_range_loop)] // index loops mirror trained tensor layout

use std::sync::OnceLock;

use image::imageops::FilterType;
use image::ImageReader;

pub const INPUT: u32 = 96;
pub const N_HEADS: usize = 3; // gore, nudity, juvenile
pub const IDX_GORE: usize = 0;
pub const IDX_NUDITY: usize = 1;
pub const IDX_JUVENILE: usize = 2;

static MODEL: OnceLock<Option<ImageNn>> = OnceLock::new();

/// Raw 96x96 RGB buffer (row-major, interleaved), normalized [0,1].
pub type Rgb96 = [[[f32; INPUT as usize]; INPUT as usize]; 3];

/// Scores for the three image heads, each [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageNnScores {
    pub gore: f32,
    pub nudity: f32,
    pub juvenile: f32,
}

/// Composed moderation signal from the image NN.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageNnVerdict {
    pub is_gore: bool,
    pub csam_risk: bool,
    pub csam_risk_score: f32,
    pub scores: ImageNnScores,
}

/// Trained image NN (weights parsed from `SOSIMG1` mlpack).
pub struct ImageNn {
    c1: usize,
    c2: usize,
    c3: usize,
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
    w3: Vec<f32>,
    b3: Vec<f32>,
    wh: Vec<f32>,
    bh: Vec<f32>,
    /// Per-head trained-at-export flag. A head with an all-zero fc row AND
    /// zero bias wasn't trained (the exporter zeroes absent heads out) and
    /// must contribute nothing — its sigmoid output is forced to 0.0 so it
    /// can never fire a moderation signal.
    heads_present: [bool; N_HEADS],
}

impl ImageNn {
    /// Parses the `SOSIMG1` binary produced by `ml/scripts/common.py`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        const MAGIC: &[u8] = b"SOSIMG1";
        // v2 = concat(GAP,GMP) pooling (64 fc inputs). v1 (GAP-only) mlpacks
        // are rejected so an old-layout asset never misparses as the new.
        if bytes.len() < MAGIC.len() + 1 || &bytes[..MAGIC.len()] != MAGIC {
            return Err("bad image mlpack magic".to_string());
        }
        if bytes[MAGIC.len()] != 2 {
            return Err("bad image mlpack version".to_string());
        }
        let mut off = MAGIC.len() + 1;
        macro_rules! rd_u32 {
            () => {{
                if off + 4 > bytes.len() {
                    return Err("image mlpack truncated (u32)".to_string());
                }
                let v = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
                off += 4;
                v as usize
            }};
        }
        macro_rules! rd_f32s {
            ($n:expr) => {{
                let n = $n;
                let byte_len = n * 4;
                if off + byte_len > bytes.len() {
                    return Err("image mlpack truncated (f32s)".to_string());
                }
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()));
                    off += 4;
                }
                v
            }};
        }
        let input = rd_u32!();
        let in_c = rd_u32!();
        let c1 = rd_u32!();
        let c2 = rd_u32!();
        let c3 = rd_u32!();
        let n_heads = rd_u32!();
        if input != INPUT as usize || in_c != 3 || n_heads != N_HEADS {
            return Err("image mlpack unexpected arch".to_string());
        }
        let w1 = rd_f32s!(c1 * 3 * 3 * 3);
        let b1 = rd_f32s!(c1);
        let w2 = rd_f32s!(c2 * 3 * 3 * c1);
        let b2 = rd_f32s!(c2);
        let w3 = rd_f32s!(c3 * 3 * 3 * c2);
        let b3 = rd_f32s!(c3);
        let wh = rd_f32s!(2 * c3 * n_heads);
        let bh = rd_f32s!(n_heads);
        let mut heads_present = [false; N_HEADS];
        for k in 0..n_heads {
            // Exporter zeroes absent heads (all-zero fc row + zero bias).
            // Kaiming init never produces exact zeros, so this is unambiguous.
            // fc row spans the 2*c3 pooled slots (avg + max per channel).
            heads_present[k] = bh[k] != 0.0 || (0..2 * c3).any(|s| wh[s * n_heads + k] != 0.0);
        }
        Ok(Self {
            c1,
            c2,
            c3,
            w1,
            b1,
            w2,
            b2,
            w3,
            b3,
            wh,
            bh,
            heads_present,
        })
    }

    fn forward(&self, img: &Rgb96) -> [f32; N_HEADS] {
        let mut h = conv3x3s2(&img[..], &self.w1, &self.b1, 3, self.c1, INPUT);
        relu(&mut h, (INPUT / 2) as usize);
        h = conv3x3s2(&h, &self.w2, &self.b2, self.c1, self.c2, INPUT / 2);
        relu(&mut h, (INPUT / 4) as usize);
        h = conv3x3s2(&h, &self.w3, &self.b3, self.c2, self.c3, INPUT / 4);
        relu(&mut h, (INPUT / 8) as usize);

        // concat(GAP, GMP) over 12x12: slot 2c = avg(ch c), 2c+1 = max(ch c).
        let step = (INPUT / 8) as usize;
        let mut pooled = [0.0f32; 2 * 32];
        for c in 0..self.c3 {
            let mut acc = 0.0f32;
            let mut mx = f32::NEG_INFINITY;
            for y in 0..step {
                for x in 0..step {
                    let v = h[c][y][x];
                    acc += v;
                    if v > mx {
                        mx = v;
                    }
                }
            }
            pooled[2 * c] = acc / (step * step) as f32;
            pooled[2 * c + 1] = mx;
        }
        let mut out = [0.0f32; N_HEADS];
        for (k, o) in out.iter_mut().enumerate() {
            if !self.heads_present[k] {
                // Untrained head (absent at export): hard 0.0, never a signal.
                *o = 0.0;
                continue;
            }
            let mut acc = self.bh[k];
            for c in 0..2 * self.c3 {
                acc += self.wh[c * N_HEADS + k] * pooled[c];
            }
            *o = sigmoid(acc);
        }
        out
    }

    /// Classifies a decoded 96x96 RGB image.
    pub fn classify(&self, img: &Rgb96) -> ImageNnScores {
        let a = self.forward(img);
        ImageNnScores {
            gore: a[IDX_GORE],
            nudity: a[IDX_NUDITY],
            juvenile: a[IDX_JUVENILE],
        }
    }
}

/// 3x3 stride-2 conv over a [c][y][x] f32 volume; returns output planes.
/// The volume is dynamic-length (out_c channels) — the trained asset uses
/// c1=16 / c2=32 / c3=32, far beyond the fixed 3-channel image input.
fn conv3x3s2(
    input: &[[[f32; INPUT as usize]; INPUT as usize]],
    w: &[f32],
    b: &[f32],
    in_c: usize,
    out_c: usize,
    size: u32,
) -> Vec<[[f32; INPUT as usize]; INPUT as usize]> {
    let s = size as usize;
    let out_s = (s / 2).max(1);
    let mut out = vec![[[0.0f32; INPUT as usize]; INPUT as usize]; out_c];
    for oc in 0..out_c {
        for oy in 0..out_s {
            for ox in 0..out_s {
                let mut acc = b[oc];
                let base_y = oy * 2;
                let base_x = ox * 2;
                for ic in 0..in_c {
                    for ky in 0..3 {
                        for kx in 0..3 {
                            let iy = base_y + ky;
                            let ix = base_x + kx;
                            // zero pad at edges
                            if iy < s && ix < s {
                                let wi = ((ic * 3 + ky) * 3 + kx) * out_c + oc;
                                acc += w[wi] * input[ic][iy][ix];
                            }
                        }
                    }
                }
                out[oc][oy][ox] = acc;
            }
        }
    }
    out
}

fn relu(v: &mut [[[f32; INPUT as usize]; INPUT as usize]], size: usize) {
    for c in 0..v.len() {
        for y in 0..size {
            for x in 0..size {
                if v[c][y][x] < 0.0 {
                    v[c][y][x] = 0.0;
                }
            }
        }
    }
}

#[inline]
fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

fn get_model() -> &'static Option<ImageNn> {
    MODEL.get_or_init(|| {
        const ASSET: &[u8] = include_bytes!("../assets/image_moderation_v1.nn");
        // Distinguish "asset placeholder" from a real model: an all-zero
        // payload (or presence marker) means "not trained yet". The exporter
        // writes a single 0x00 byte until real weights exist.
        const PLACEHOLDER: &[u8] = &[0x00];
        if ASSET == PLACEHOLDER || ASSET.is_empty() {
            return None;
        }
        ImageNn::from_bytes(ASSET).ok()
    })
}

/// Whether trained image weights exist and loaded.
pub fn image_nn_available() -> bool {
    get_model().is_some()
}

/// The loaded image NN (None while the asset is the untrained placeholder).
/// Enables the video pipeline (`video_nn`) to run the same model per frame.
pub fn image_nn_model() -> Option<&'static ImageNn> {
    get_model().as_ref()
}

/// Decodes image bytes with hard caps on hostile input (4096px / 64 MiB).
pub fn decode_capped(bytes: &[u8]) -> Option<image::DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.limits(limits);
    reader.decode().ok()
}

/// Converts a decoded RGB image into the normalized 96x96 network input.
pub fn rgb_to_rgb96(rgb: &image::RgbImage) -> Rgb96 {
    let small = image::imageops::resize(rgb, INPUT, INPUT, FilterType::Triangle);
    let mut img: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
    for (y, row) in small.rows().enumerate() {
        for (x, px) in row.enumerate() {
            img[0][y][x] = px[0] as f32 / 255.0;
            img[1][y][x] = px[1] as f32 / 255.0;
            img[2][y][x] = px[2] as f32 / 255.0;
        }
    }
    img
}

/// Classifies a decoded RGB buffer (any size) with the trained image NN.
/// Returns `None` when no trained model (placeholder asset) is loaded.
pub fn classify_rgb(rgb: &image::RgbImage) -> Option<ImageNnScores> {
    let model = image_nn_model()?;
    Some(model.classify(&rgb_to_rgb96(rgb)))
}

/// Decodes bytes → 96x96 RGB (hostile-input capped) and classifies.
/// Returns `None` when no trained model, no image, or decode failure.
pub fn classify_image_bytes(bytes: &[u8]) -> Option<ImageNnScores> {
    let dyn_img = decode_capped(bytes)?;
    classify_rgb(&dyn_img.to_rgb8())
}

/// Composes head scores into a moderation verdict.
///
/// - `is_gore`: gore head high-confidence.
/// - `csam_risk`: nudity AND juvenile both high (AND-gated so adult nudity
///   alone never flags). Only feeds the report/review gate downstream.
pub fn compose_verdict(s: ImageNnScores) -> ImageNnVerdict {
    let is_gore = s.gore >= 0.55;
    let csam_nudge = (s.nudity - 0.60).max(0.0);
    let juv_nudge = (s.juvenile - 0.50).max(0.0);
    let csam_risk_score = (csam_nudge * juv_nudge * 12.0).clamp(0.0, 1.0);
    ImageNnVerdict {
        is_gore,
        csam_risk: csam_risk_score >= 0.25,
        csam_risk_score,
        scores: s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::identity_op, clippy::erasing_op)]
    fn synthetic_model() -> ImageNn {
        // Builds a byte-valid model with trivial weights to exercise the
        // forward pass + composition logic (not a trained model).
        // The literal channel-offset arithmetic mirrors the training exporter.
        let c1 = 2;
        let c2 = 2;
        let c3 = 2;
        let mut w1 = vec![0.0f32; c1 * 27];
        let mut b1 = vec![0.0f32; c1];
        let mut w2 = vec![0.0f32; c2 * 9 * c1];
        let mut b2 = vec![0.0f32; c2];
        let mut w3 = vec![0.0f32; c3 * 9 * c2];
        let mut b3 = vec![0.0f32; c3];
        let mut wh = vec![0.0f32; 2 * c3 * 3];
        let mut bh = vec![0.0f32; 3];
        let _ = (&mut b1, &mut b2, &mut b3, &mut w1, &mut w2, &mut w3);
        // red channel -> gore; middle gray -> nudity+juv
        for ky in 0..3 {
            for kx in 0..3 {
                w1[((0 * 3 + ky) * 3 + kx) * c1 + 0] = 0.12;
                w1[((1 * 3 + ky) * 3 + kx) * c1 + 1] = 0.12;
            }
        }
        b1[0] = 0.0;
        b1[1] = 0.0;
        for oc in 0..c2 {
            for ic in 0..c1 {
                for ky in 0..3 {
                    for kx in 0..3 {
                        w2[((ic * 3 + ky) * 3 + kx) * c2 + oc] = 0.05;
                    }
                }
            }
        }
        for oc in 0..c3 {
            for ic in 0..c2 {
                for ky in 0..3 {
                    for kx in 0..3 {
                        w3[((ic * 3 + ky) * 3 + kx) * c3 + oc] = 0.05;
                    }
                }
            }
        }
        // slots 2c = avg(ch c), 2c+1 = max(ch c). Wire: avg(ch0) -> gore,
        // avg(ch1) -> nudity + juvenile.
        wh[0 * N_HEADS + 0] = 1.0; // avg ch0 -> gore
        wh[2 * N_HEADS + 1] = 1.0; // avg ch1 -> nudity
        wh[2 * N_HEADS + 2] = 1.0; // avg ch1 -> juvenile
        bh = vec![0.0, 0.0, 0.0];
        ImageNn {
            c1,
            c2,
            c3,
            w1,
            b1,
            w2,
            b2,
            w3,
            b3,
            wh,
            bh,
            heads_present: [true; N_HEADS],
        }
    }

    #[test]
    fn from_bytes_detects_absent_head_from_zeroed_slice() {
        let mut m = synthetic_model();
        for s in 0..2 * m.c3 {
            m.wh[s * N_HEADS + IDX_GORE] = 0.0;
        }
        m.bh[IDX_GORE] = 0.0;
        // Serialize like the exporter (magic, version 2, dims, arrays).
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"SOSIMG1");
        bytes.push(2u8);
        for v in [INPUT as usize, 3usize, m.c1, m.c2, m.c3, N_HEADS] {
            bytes.extend_from_slice(&(v as u32).to_le_bytes());
        }
        let all: Vec<f32> = [
            m.w1.clone(),
            m.b1.clone(),
            m.w2.clone(),
            m.b2.clone(),
            m.w3.clone(),
            m.b3.clone(),
            m.wh.clone(),
            m.bh.clone(),
        ]
        .concat();
        for x in &all {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        let parsed = ImageNn::from_bytes(&bytes).expect("valid mlpack");
        assert!(!parsed.heads_present[IDX_GORE], "zeroed gore row = absent");
        assert!(parsed.heads_present[IDX_NUDITY]);
        assert!(parsed.heads_present[IDX_JUVENILE]);
        assert_eq!(
            parsed
                .classify(&[[[0.0f32; INPUT as usize]; INPUT as usize]; 3])
                .gore,
            0.0
        );
    }

    #[test]
    fn real_asset_loads_with_absent_gore_head() {
        // The shipped mlpack is the trained GMP model (mlpack v2). Gore has no
        // cleared corpus, so its exported fc row + bias are all-zero → Rust
        // marks the head absent, forces its output to 0.0, and it can never
        // block (chrominance + hash stay the only gore block sources).
        assert!(
            image_nn_available(),
            "shipped image mlpack must be a real model"
        );
        let m = image_nn_model().expect("trained model loaded");
        assert!(
            !m.heads_present[IDX_GORE],
            "gore head must be absent (no corpus)"
        );
        assert!(m.heads_present[IDX_NUDITY], "nudity head must be trained");
        assert!(
            m.heads_present[IDX_JUVENILE],
            "juvenile head must be trained"
        );
        // Untrained/absent heads still score exactly 0.0...
        let zeros = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
        assert_eq!(m.classify(&zeros).gore, 0.0);
        // ...and undecodable bytes still degrade to None.
        assert_eq!(classify_image_bytes(&[]), None);
        assert_eq!(classify_image_bytes(&[0u8; 512]), None);
    }

    #[test]
    fn forward_runs_on_red_image() {
        let m = synthetic_model();
        let mut img: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
        for y in 0..INPUT as usize {
            for x in 0..INPUT as usize {
                img[0][y][x] = 1.0; // red
            }
        }
        let s = m.classify(&img);
        assert!(s.gore > 0.5);
    }

    #[test]
    fn absent_gore_head_scores_zero_and_never_blocks() {
        let mut m = synthetic_model();
        // Mirror the exporter's missing-head layout: zero fc row + bias.
        for s in 0..2 * m.c3 {
            m.wh[s * N_HEADS + IDX_GORE] = 0.0;
        }
        m.bh[IDX_GORE] = 0.0;
        m.heads_present[IDX_GORE] = false;
        // Even a maximally-gore-looking image must not fire the gore head.
        let mut img: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
        for y in 0..INPUT as usize {
            for x in 0..INPUT as usize {
                img[0][y][x] = 1.0;
            }
        }
        let s = m.classify(&img);
        assert_eq!(s.gore, 0.0, "absent gore head must be scored 0");
        let v = compose_verdict(s);
        assert!(!v.is_gore, "absent gore head must never block");
        // Nudity + juvenile heads still resolve from the shared convs.
        let s2 = m.classify(&img);
        assert!(s2.juvenile >= 0.0 && s2.nudity >= 0.0);
    }

    #[test]
    fn adult_nudity_alone_never_csam() {
        let v = compose_verdict(ImageNnScores {
            gore: 0.1,
            nudity: 0.99,
            juvenile: 0.1,
        });
        assert!(!v.csam_risk, "adult nudity must not produce CSAM risk");
        assert!(!v.is_gore);
    }

    #[test]
    fn nudity_and_juvenile_compose_csam_risk() {
        let v = compose_verdict(ImageNnScores {
            gore: 0.2,
            nudity: 0.95,
            juvenile: 0.9,
        });
        assert!(v.csam_risk);
        assert!(v.csam_risk_score > 0.5);
    }
}
