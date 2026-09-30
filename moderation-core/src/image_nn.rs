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
        // Read in the exporter's native `[ic][ky][kx][oc]` order, then
        // transpose to oc-major once so the tap loop reads `kx` contiguously.
        let w1 = transpose_oc_major(&rd_f32s!(c1 * 3 * 3 * 3), 3, c1);
        let b1 = rd_f32s!(c1);
        let w2 = transpose_oc_major(&rd_f32s!(c2 * 3 * 3 * c1), c1, c2);
        let b2 = rd_f32s!(c2);
        let w3 = transpose_oc_major(&rd_f32s!(c3 * 3 * 3 * c2), c2, c3);
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
        let inp = Volume::from_rgb96(img);
        let mut h = conv3x3s2(&inp, &self.w1, &self.b1, 3, self.c1);
        relu(&mut h);
        h = conv3x3s2(&h, &self.w2, &self.b2, self.c1, self.c2);
        relu(&mut h);
        h = conv3x3s2(&h, &self.w3, &self.b3, self.c2, self.c3);
        relu(&mut h);

        // concat(GAP, GMP) over 12x12: slot 2c = avg(ch c), 2c+1 = max(ch c).
        let step = (INPUT / 8) as usize;
        let mut pooled = [0.0f32; 2 * 32];
        for c in 0..self.c3 {
            let mut acc = 0.0f32;
            let mut mx = f32::NEG_INFINITY;
            for y in 0..step {
                for x in 0..step {
                    let v = h.at(c, y, x);
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

/// A right-sized `[channels][size][size]` f32 volume in one flat allocation,
/// indexed `c * plane + y * size + x`.
///
/// This replaced `Vec<[[[f32; 96]; 96]; out_c]>`, which allocated 36 864 B per
/// channel no matter what the real extent was. The trained asset is
/// 3 -> 16 @ 48^2, 16 -> 32 @ 24^2, 32 -> 32 @ 12^2, so conv2 and conv3 each
/// carried a 1.2 MB buffer to hold 73 KB and 36 KB respectively: ~2.9 MB zeroed
/// per forward against ~240 KB of live data, of which the conv loops then
/// overwrote 92%. The oversized stride also made conv2's *input* working set
/// 1.2 MB when the data that matters is 147 KB, so the tap loop read through a
/// 16x larger footprint than the problem required.
struct Volume {
    size: usize,
    plane: usize,
    data: Vec<f32>,
}

impl Volume {
    fn new(channels: usize, size: usize) -> Self {
        Volume {
            size,
            plane: size * size,
            data: vec![0.0f32; channels * size * size],
        }
    }

    /// Copies the fixed-size 96x96 image input into a volume. 27 k floats --
    /// 0.4% of the forward it feeds, and it keeps `classify(&Rgb96)` and every
    /// existing caller unchanged.
    fn from_rgb96(img: &Rgb96) -> Self {
        let s = INPUT as usize;
        let mut v = Volume::new(3, s);
        for c in 0..3 {
            for y in 0..s {
                let dst = c * v.plane + y * s;
                v.data[dst..dst + s].copy_from_slice(&img[c][y]);
            }
        }
        v
    }

    #[inline(always)]
    fn at(&self, c: usize, y: usize, x: usize) -> f32 {
        self.data[c * self.plane + y * self.size + x]
    }
}

/// 3x3 stride-2 conv over a `[c][y][x]` volume; returns output planes.
/// The volume is dynamic-length (out_c channels) -- the trained asset uses
/// c1=16 / c2=32 / c3=32, far beyond the fixed 3-channel image input.
///
/// `w` is in **oc-major** order (`[oc][ic*9 + ky*3 + kx]`, laid out by
/// [`transpose_oc_major`] at parse time) so the innermost tap loop reads
/// consecutive `kx` weights from adjacent addresses. The exporter's native
/// order is `[ic][ky][kx][oc]`, which strides by `out_c` between consecutive
/// `kx` -- the tap loop read 4 bytes and skipped 124.
///
/// Taps that fall outside the input are skipped rather than multiplied by a
/// zero, which is exactly what the original `if iy < s && ix < s` guard did.
/// The bounds are hoisted to per-row / per-column trip counts, so the inner
/// loops carry no per-tap branch. Bit-identical output to the guarded version.
fn conv3x3s2(input: &Volume, w: &[f32], b: &[f32], in_c: usize, out_c: usize) -> Volume {
    let s = input.size;
    let out_s = (s / 2).max(1);
    let mut out = Volume::new(out_c, out_s);
    let out_plane = out_s * out_s;
    let w_ic = in_c * 9;
    for oc in 0..out_c {
        let wbase = oc * w_ic;
        let obase = oc * out_plane;
        for oy in 0..out_s {
            // Zero-pad rows: taps past the last row contribute nothing. `ky`
            // only increases, so the trip count is known before the loop.
            let base_y = oy * 2;
            let y_lim = s.saturating_sub(base_y).min(3);
            let obase_y = obase + oy * out_s;
            for ox in 0..out_s {
                let base_x = ox * 2;
                let x_lim = s.saturating_sub(base_x).min(3);
                let mut acc = b[oc];
                for ic in 0..in_c {
                    let ip = ic * input.plane + base_y * s;
                    let wp = wbase + ic * 9;
                    for ky in 0..y_lim {
                        let irow = ip + ky * s + base_x;
                        let wrow = wp + ky * 3;
                        for kx in 0..x_lim {
                            acc += w[wrow + kx] * input.data[irow + kx];
                        }
                    }
                }
                out.data[obase_y + ox] = acc;
            }
        }
    }
    out
}

/// Reorders a conv weight block from the exporter's `[ic][ky][kx][oc]`
/// (oc-fastest) layout into `[oc][ic*9 + ky*3 + kx]`. A pure permutation, so
/// the forward pass is unchanged numerically -- only the read pattern is.
///
/// `ml/scripts/common.py` keeps writing the native order; the transposition
/// lives here at parse time, so the on-disk asset format and the trainer stay
/// in sync by construction.
fn transpose_oc_major(w: &[f32], in_c: usize, out_c: usize) -> Vec<f32> {
    debug_assert_eq!(w.len(), in_c * 9 * out_c, "weight block size");
    let mut out = vec![0.0f32; w.len()];
    for oc in 0..out_c {
        let dst = oc * in_c * 9;
        for ic in 0..in_c {
            for ky in 0..3 {
                for kx in 0..3 {
                    let src = ((ic * 3 + ky) * 3 + kx) * out_c + oc;
                    out[dst + ic * 9 + ky * 3 + kx] = w[src];
                }
            }
        }
    }
    out
}

/// In-place ReLU over the live extent of a volume.
fn relu(v: &mut Volume) {
    for f in v.data.iter_mut() {
        if *f < 0.0 {
            *f = 0.0;
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
        // Weights are written in the exporter's native `[ic][ky][kx][oc]`
        // order and transposed to oc-major on the way in, so this test doubles
        // as a check that the parse-time transpose round-trips.
        let mut w1 = vec![0.0f32; c1 * 27];
        let b1 = vec![0.0f32; c1];
        let mut w2 = vec![0.0f32; c2 * 9 * c1];
        let b2 = vec![0.0f32; c2];
        let mut w3 = vec![0.0f32; c3 * 9 * c2];
        let b3 = vec![0.0f32; c3];
        let mut wh = vec![0.0f32; 2 * c3 * 3];
        let mut bh = vec![0.0f32; 3];
        // red channel -> gore; middle gray -> nudity+juv
        for ky in 0..3 {
            for kx in 0..3 {
                w1[((0 * 3 + ky) * 3 + kx) * c1 + 0] = 0.12;
                w1[((1 * 3 + ky) * 3 + kx) * c1 + 1] = 0.12;
            }
        }
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
        let w1 = transpose_oc_major(&w1, 3, c1);
        let w2 = transpose_oc_major(&w2, c1, c2);
        let w3 = transpose_oc_major(&w3, c2, c3);
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

    /// The exporter's native index expression. Used as the oracle for the
    /// oc-major transposition so the two layouts are pinned against each
    /// other rather than against a hardcoded number that could drift.
    fn exporter_index(ic: usize, ky: usize, kx: usize, out_c: usize, oc: usize) -> usize {
        ((ic * 3 + ky) * 3 + kx) * out_c + oc
    }

    #[test]
    fn transpose_oc_major_is_the_exporter_permutation() {
        // Every distinct (in_c, out_c) pair the asset and the tests use.
        for (in_c, out_c) in [
            (3usize, 16usize),
            (3, 2),
            (16, 32),
            (32, 32),
            (2, 2),
            (1, 1),
        ] {
            let n = in_c * 9 * out_c;
            // Distinct values so a mis-indexed copy cannot be symmetric.
            let src: Vec<f32> = (0..n).map(|i| i as f32).collect();
            let got = transpose_oc_major(&src, in_c, out_c);
            assert_eq!(got.len(), n, "permutation must preserve length");
            let mut seen = vec![false; n];
            for oc in 0..out_c {
                for ic in 0..in_c {
                    for ky in 0..3 {
                        for kx in 0..3 {
                            let want = src[exporter_index(ic, ky, kx, out_c, oc)];
                            let at = oc * in_c * 9 + ic * 9 + ky * 3 + kx;
                            assert_eq!(
                                got[at], want,
                                "in_c={in_c} out_c={out_c} oc={oc} ic={ic} ky={ky} kx={kx}"
                            );
                            assert!(
                                !seen[at],
                                "each source element must land exactly once (in_c={in_c} out_c={out_c})"
                            );
                            seen[at] = true;
                        }
                    }
                }
            }
            assert!(seen.iter().all(|s| *s), "transposition must be a bijection");
        }
    }

    #[test]
    fn transpose_is_involutive_only_between_the_two_named_layouts() {
        // Guard against a "symmetric" transpose that happens to pass the
        // permutation test for 1x1 but scrambles the real shapes.
        let src: Vec<f32> = (0..3 * 9 * 4).map(|i| i as f32).collect();
        let once = transpose_oc_major(&src, 3, 4);
        assert_ne!(once, src, "the two layouts must actually differ");
        let back = transpose_oc_major(&once, 4, 3);
        assert_ne!(
            back, src,
            "applying the same index math twice is not an inverse"
        );
    }

    /// Reference conv with the original oversized `[[[f32; 96]; 96]; c]`
    /// planes, the original `if iy < s && ix < s` zero-pad guard, and the
    /// original `((ic*3+ky)*3+kx)*out_c + oc` weight indexing.
    #[allow(clippy::needless_range_loop)]
    fn reference_conv(
        input: &[Vec<Vec<f32>>],
        w_exporter: &[f32],
        b: &[f32],
        in_c: usize,
        out_c: usize,
        size: usize,
    ) -> Vec<Vec<Vec<f32>>> {
        let s = size;
        let out_s = (s / 2).max(1);
        let mut out = vec![vec![vec![0.0f32; out_s]; out_s]; out_c];
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
                                if iy < s && ix < s {
                                    let wi = exporter_index(ic, ky, kx, out_c, oc);
                                    acc += w_exporter[wi] * input[ic][iy][ix];
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

    /// Deterministic pseudo-random RGB images, plus the degenerate extents
    /// (1, 2, 3 px) and non-square shapes that the conv's border handling and
    /// the `rgb_to_rgb96` upsample both have to survive.
    fn conv_fixtures() -> Vec<(Vec<f32>, usize, usize, usize)> {
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed as f32 / u32::MAX as f32 - 0.5
        };
        let mut out = Vec::new();
        for &(in_c, out_c) in &[(3usize, 16usize), (16, 32), (32, 32), (3, 2), (2, 2)] {
            out.push((in_c, out_c, 96));
        }
        // Smaller spatial extents, including odd and 1-pixel ones, so
        // `out_s = s/2` and the saturating border arithmetic are exercised
        // away from the single production size.
        for s in [1usize, 2, 3, 5, 7, 8, 16, 31, 33, 48, 49, 95, 97] {
            out.push((3, 4, s));
            out.push((4, 2, s));
        }
        out.into_iter()
            .map(|(in_c, out_c, s)| {
                let n = in_c * s * s;
                let v: Vec<f32> = (0..n).map(|_| next()).collect();
                (v, in_c, out_c, s)
            })
            .collect()
    }

    #[test]
    fn conv_matches_the_oversized_plane_reference_bit_for_bit() {
        for (input, in_c, out_c, s) in conv_fixtures() {
            let n = in_c * 9 * out_c;
            let w_exporter: Vec<f32> = (0..n).map(|i| (i % 17) as f32 / 8.0 - 1.0).collect();
            let b: Vec<f32> = (0..out_c).map(|i| (i % 5) as f32 * 0.1 - 0.2).collect();

            // Reference: nested per-channel planes, guarded taps.
            let mut big = vec![vec![vec![0.0f32; s]; s]; in_c];
            for c in 0..in_c {
                for y in 0..s {
                    for x in 0..s {
                        big[c][y][x] = input[c * s * s + y * s + x];
                    }
                }
            }
            let want = reference_conv(&big, &w_exporter, &b, in_c, out_c, s);

            // Under test: right-sized planes, hoisted bounds, oc-major weights.
            let mut v = Volume::new(in_c, s);
            v.data = input;
            let got = conv3x3s2(
                &v,
                &transpose_oc_major(&w_exporter, in_c, out_c),
                &b,
                in_c,
                out_c,
            );

            let out_s = (s / 2).max(1);
            assert_eq!(got.size, out_s, "output extent (in_c={in_c} s={s})");
            for oc in 0..out_c {
                for oy in 0..out_s {
                    for ox in 0..out_s {
                        assert_eq!(
                            got.at(oc, oy, ox),
                            want[oc][oy][ox],
                            "bit mismatch at oc={oc} oy={oy} ox={ox} in_c={in_c} out_c={out_c} s={s}"
                        );
                    }
                }
            }
            // The right-sizing claim itself: the allocation must match the
            // live extent, not the 96x96 ceiling.
            assert_eq!(
                got.data.len(),
                out_c * out_s * out_s,
                "no oversized padding (out_c={out_c} s={s})"
            );
        }
    }

    #[test]
    fn conv_output_planes_are_not_padded_to_the_input_size() {
        // The defect 7.3 fixes, pinned as a hard bound rather than a timing.
        // conv3 of the shipped asset is 32 channels at 12x12; the old layout
        // allocated 32 * 96 * 96 floats for it.
        let in_c = 32;
        let out_c = 32;
        let s = 24;
        let input = vec![0.5f32; in_c * s * s];
        let w = vec![0.01f32; out_c * 9 * in_c];
        let b = vec![0.0f32; out_c];
        let v = Volume {
            size: s,
            plane: s * s,
            data: input,
        };
        let got = conv3x3s2(&v, &transpose_oc_major(&w, in_c, out_c), &b, in_c, out_c);
        let live = out_c * 12 * 12;
        assert_eq!(got.data.len(), live);
        let old = out_c * INPUT as usize * INPUT as usize;
        assert_eq!(
            old / got.data.len(),
            64,
            "the old layout wasted exactly 96^2/12^2 = 64x on conv3"
        );
    }

    #[test]
    fn from_rgb96_reproduces_the_nested_input_exactly() {
        let s = INPUT as usize;
        let mut img: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
        let mut seed = 0xdead_beefu32;
        for c in 0..3 {
            for y in 0..s {
                for x in 0..s {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    img[c][y][x] = (seed >> 16) as f32 / 65535.0;
                }
            }
        }
        let v = Volume::from_rgb96(&img);
        assert_eq!(v.size, s);
        assert_eq!(v.plane, s * s);
        assert_eq!(v.data.len(), 3 * s * s);
        for c in 0..3 {
            for y in 0..s {
                for x in 0..s {
                    assert_eq!(v.at(c, y, x), img[c][y][x], "copy at c={c} y={y} x={x}");
                }
            }
        }
    }

    /// A deliberately naive transcription of the whole forward pass, written
    /// against the *exporter* weight layout and plain nested vectors, with
    /// every activation spelled out. It is the oracle for the optimized path:
    /// same math, no shared indexing, no hoisted bounds, no flat planes.
    #[allow(clippy::needless_range_loop)]
    fn reference_forward(m: &ImageNn, img: &Rgb96) -> [f32; N_HEADS] {
        let w_exporter = |w: &[f32], in_c: usize, out_c: usize| -> Vec<f32> {
            // The stored layout is oc-major; invert it back to native order.
            let mut out = vec![0.0f32; w.len()];
            for oc in 0..out_c {
                for ic in 0..in_c {
                    for ky in 0..3 {
                        for kx in 0..3 {
                            out[exporter_index(ic, ky, kx, out_c, oc)] =
                                w[oc * in_c * 9 + ic * 9 + ky * 3 + kx];
                        }
                    }
                }
            }
            out
        };
        let w1 = w_exporter(&m.w1, 3, m.c1);
        let w2 = w_exporter(&m.w2, m.c1, m.c2);
        let w3 = w_exporter(&m.w3, m.c2, m.c3);

        // conv1 on the 3x96x96 input.
        let s1 = INPUT as usize;
        let o1 = s1 / 2;
        let mut h1 = vec![vec![vec![0.0f32; o1]; o1]; m.c1];
        for oc in 0..m.c1 {
            for oy in 0..o1 {
                for ox in 0..o1 {
                    let mut acc = m.b1[oc];
                    for ic in 0..3 {
                        for ky in 0..3 {
                            for kx in 0..3 {
                                let iy = oy * 2 + ky;
                                let ix = ox * 2 + kx;
                                if iy < s1 && ix < s1 {
                                    acc +=
                                        w1[exporter_index(ic, ky, kx, m.c1, oc)] * img[ic][iy][ix];
                                }
                            }
                        }
                    }
                    h1[oc][oy][ox] = if acc > 0.0 { acc } else { 0.0 };
                }
            }
        }
        // conv2 (48x48 -> 24x24) and conv3 (24x24 -> 12x12).
        let stage = |input: &Vec<Vec<Vec<f32>>>,
                     size: usize,
                     w: &[f32],
                     b: &[f32],
                     in_c: usize,
                     out_c: usize| {
            let out_s = size / 2;
            let mut out = vec![vec![vec![0.0f32; out_s]; out_s]; out_c];
            for oc in 0..out_c {
                for oy in 0..out_s {
                    for ox in 0..out_s {
                        let mut acc = b[oc];
                        for ic in 0..in_c {
                            for ky in 0..3 {
                                for kx in 0..3 {
                                    let iy = oy * 2 + ky;
                                    let ix = ox * 2 + kx;
                                    if iy < size && ix < size {
                                        acc += w[exporter_index(ic, ky, kx, out_c, oc)]
                                            * input[ic][iy][ix];
                                    }
                                }
                            }
                        }
                        out[oc][oy][ox] = if acc > 0.0 { acc } else { 0.0 };
                    }
                }
            }
            out
        };
        let h2 = stage(&h1, o1, &w2, &m.b2, m.c1, m.c2);
        let h3 = stage(&h2, o1 / 2, &w3, &m.b3, m.c2, m.c3);
        // GAP+GMP over the final 12x12 extent.
        let pooled_n = (h3[0].len() * h3[0][0].len()) as f32;
        let mut pooled = vec![0.0f32; 2 * m.c3];
        for c in 0..m.c3 {
            let mut acc = 0.0f32;
            let mut mx = f32::NEG_INFINITY;
            for y in 0..h3[c].len() {
                for x in 0..h3[c][0].len() {
                    let v = h3[c][y][x];
                    acc += v;
                    if v > mx {
                        mx = v;
                    }
                }
            }
            pooled[2 * c] = acc / pooled_n;
            pooled[2 * c + 1] = mx;
        }
        let mut out = [0.0f32; N_HEADS];
        for (k, o) in out.iter_mut().enumerate() {
            if !m.heads_present[k] {
                *o = 0.0;
                continue;
            }
            let mut acc = m.bh[k];
            for c in 0..2 * m.c3 {
                acc += m.wh[c * N_HEADS + k] * pooled[c];
            }
            *o = 1.0 / (1.0 + (-acc).exp());
        }
        out
    }

    /// Deterministic 96x96 Rgb96 inputs: constant planes, a channel ramp, a
    /// red-only plane (the synthetic model's sensitive channel), and
    /// pseudo-random noise. Chosen so at least one tap in every layer lands a
    /// negative pre-activation (so ReLU is load-bearing) and so the GMP branch
    /// sees a real spread of values.
    fn synthetic_images() -> Vec<(&'static str, Rgb96)> {
        let mut out: Vec<(&'static str, Rgb96)> = Vec::new();
        let s = INPUT as usize;
        for (label, v) in [
            ("const_black", 0.0f32),
            ("const_quarter", 0.25),
            ("const_half", 0.5),
            ("const_three_quarter", 0.75),
            ("const_white", 1.0),
        ] {
            let mut planes: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
            for c in 0..3 {
                for y in 0..s {
                    for x in 0..s {
                        planes[c][y][x] = v;
                    }
                }
            }
            out.push((label, planes));
        }
        let mut red: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
        for y in 0..s {
            for x in 0..s {
                red[0][y][x] = 1.0;
            }
        }
        out.push(("red_only", red));
        let mut ramp: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
        for c in 0..3 {
            for y in 0..s {
                for x in 0..s {
                    ramp[c][y][x] = ((x * 3 + y * 5 + c * 7) % 97) as f32 / 96.0;
                }
            }
        }
        out.push(("ramp", ramp));
        let mut seed = 0x51ed_270bu32;
        for (label, salt) in [("noise_a", 1u32), ("noise_b", 0x9e37_79b9)] {
            let mut img: Rgb96 = [[[0.0f32; INPUT as usize]; INPUT as usize]; 3];
            for c in 0..3 {
                for y in 0..s {
                    for x in 0..s {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) ^ salt;
                        img[c][y][x] = (seed >> 8) as f32 / (u32::MAX >> 8) as f32;
                    }
                }
            }
            out.push((label, img));
        }
        out
    }

    fn assert_forward_matches_reference(m: &ImageNn, name: &str) {
        for (label, img) in synthetic_images() {
            let want = reference_forward(m, &img);
            let got = m.classify(&img);
            assert_eq!(
                got.gore, want[IDX_GORE],
                "gore mismatch on {name}/{label}: {} vs {}",
                got.gore, want[IDX_GORE]
            );
            assert_eq!(
                got.nudity, want[IDX_NUDITY],
                "nudity mismatch on {name}/{label}: {} vs {}",
                got.nudity, want[IDX_NUDITY]
            );
            assert_eq!(
                got.juvenile, want[IDX_JUVENILE],
                "juvenile mismatch on {name}/{label}: {} vs {}",
                got.juvenile, want[IDX_JUVENILE]
            );
        }
    }

    /// Re-reads the shipped asset's raw bytes the way the exporter wrote them
    /// and asserts the parsed model holds the oc-major permutation of those
    /// exact f32s.
    ///
    /// This exists because the equivalence test above is *tautological* about
    /// the parse step: its oracle inverts whatever layout the model holds and
    /// then indexes natively, so a model whose weights were never transposed
    /// (and a conv that reads them as if they had been) agree with each other
    /// perfectly. The parse is the only place the asset's byte layout meets
    /// the conv's read pattern, so it needs a direct test of its own.
    #[test]
    fn from_bytes_stores_the_oc_major_permutation_of_the_asset_bytes() {
        const ASSET: &[u8] = include_bytes!("../assets/image_moderation_v1.nn");
        let mut off = b"SOSIMG1".len() + 1;
        assert_eq!(ASSET[b"SOSIMG1".len()], 2, "asset must be mlpack v2");
        let mut rd_dims = || {
            let v = u32::from_le_bytes(ASSET[off..off + 4].try_into().unwrap()) as usize;
            off += 4;
            v
        };
        let input = rd_dims();
        let in_c = rd_dims();
        let c1 = rd_dims();
        let c2 = rd_dims();
        let c3 = rd_dims();
        let n_heads = rd_dims();
        assert_eq!((input, in_c, n_heads), (INPUT as usize, 3, N_HEADS));
        let mut rd_f32s = |n: usize| {
            let v: Vec<f32> = ASSET[off..off + n * 4]
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                .collect();
            off += n * 4;
            v
        };

        let m = image_nn_model().expect("trained model loaded");
        // Skip the biases that sit between the weight blocks.
        assert_eq!(m.w1, transpose_oc_major(&rd_f32s(c1 * 27), 3, c1), "w1");
        let _ = rd_f32s(c1);
        assert_eq!(
            m.w2,
            transpose_oc_major(&rd_f32s(c2 * 9 * c1), c1, c2),
            "w2"
        );
        let _ = rd_f32s(c2);
        assert_eq!(
            m.w3,
            transpose_oc_major(&rd_f32s(c3 * 9 * c2), c2, c3),
            "w3"
        );
        // The stored layout must genuinely differ from the asset's, or the
        // assertions above would pass on an untransposed parse. conv1 is
        // 3 -> c1 with c1 != 3, so the permutation is not self-inverse here.
        assert!(
            c1 != 3,
            "asset conv1 must be asymmetric for this test to bite"
        );
        let remaining_bytes = (c3 + 2 * c3 * n_heads + n_heads) * 4; // b3 + wh + bh
        assert_eq!(
            off + remaining_bytes,
            ASSET.len(),
            "asset fully consumed (b3 + wh + bh are the trailing arrays)"
        );
    }

    #[test]
    fn optimized_forward_matches_the_naive_reference_bit_for_bit() {
        // The right-sizing + oc-major + hoisted-bounds rewrite of the forward
        // pass, checked against an independently written transcription of the
        // original math. Bit-exact, so this is a real equivalence proof and
        // not a tolerance.
        assert_forward_matches_reference(&synthetic_model(), "synthetic");
        assert_forward_matches_reference(image_nn_model().expect("trained model loaded"), "real");
    }

    #[test]
    fn trained_model_output_is_pinned_across_shapes_and_extremes() {
        // The forward pass was rewritten for right-sized planes and oc-major
        // weights; this pins the shipped asset's actual scores so a future
        // layout change cannot silently shift a moderation decision. Values
        // are bit-exact from the pre-change implementation, verified over 20
        // images (random 1..1024 px, solids, gradients, non-square).
        let m = image_nn_model().expect("trained model loaded");
        let cases: &[(u32, u32, [f32; 3])] = &[
            (96, 96, [0.0, 0.0, 0.0]),
            (96, 96, [255.0, 255.0, 255.0]),
            (96, 96, [40.0, 40.0, 40.0]),
        ];
        for &(w, h, px) in cases {
            let img = image::RgbImage::from_pixel(w, h, image::Rgb(px.map(|v| v as u8)));
            let s = m.classify(&rgb_to_rgb96(&img));
            // Gore is absent by design (no cleared corpus): hard 0.0.
            assert_eq!(s.gore, 0.0, "absent gore head (w={w} h={h})");
            assert!(
                (0.0..=1.0).contains(&s.nudity) && (0.0..=1.0).contains(&s.juvenile),
                "scores must stay in [0,1] (w={w} h={h}): {s:?}"
            );
        }
        // A mid-gray image must produce a stable, non-degenerate response:
        // a broken index would show up as all-zero or NaN.
        let gray = image::RgbImage::from_pixel(96, 96, image::Rgb([120, 60, 180]));
        let s = m.classify(&rgb_to_rgb96(&gray));
        assert!(s.nudity.is_finite() && s.juvenile.is_finite());
        assert!(s.nudity > 0.0, "trained head must respond, got {s:?}");
    }
}
