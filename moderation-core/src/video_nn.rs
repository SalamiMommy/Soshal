//! On-device video frame moderation: run the trained image CNN (plus real
//! pixel chrominance) against sampled frames of an MP4 video.
//!
//! Purely Rust pipeline — no client cooperation, works on relay-fetched
//! remote video bytes just as well as locally-produced ones:
//!
//! ```text
//! mp4 bytes
//!   → mp4parse (ISO-BMFF demux, sample table + avcC/av1C config)
//!   → evenly-spaced sync-sample selection (≤ MAX_VIDEO_FRAMES)
//!   → H.264 (rust_h264) or AV1 (rav1d-safe) decode → YUV 4:2:0
//!   → YUV→RGB downscale (≤ MAX_SCALE_DIM)
//!   → real chrominance (skin/gore ratios) + image NN per frame
//!   → VideoFramesVerdict aggregation (maxes + CSAM-risk frame count)
//! ```
//!
//! Policy (mirrors the still-image path, see `ai_media::finalize_verdict`):
//! - `gore` (chrominance OR NN) blocks.
//! - `nudity` alone is informative only — adult nudity never flags.
//! - `nudity × juvenile` composes CSAM-*risk* → report/review gate only;
//!   the hash blocklist stays the sole source of `is_csam_hazard`.
//! - The image NN yields no signal at all until real weights land (honest
//!   degradation — chrominance still runs per frame).
//!
//! Coverage: MP4 (ISO-BMFF) containers with H.264 or AV1 video tracks. Other
//! containers/codecs fall back to the existing hash + byte/chrominance path.
//! On 32-bit ARM (armeabi-v7a) AV1 frame decode is compiled out —
//! `rav1d-safe` needs nightly for `stdarch_arm_feature_detection` there — so
//! armv7 falls back to hash/chrominance for AV1 videos; H.264 still decodes.

use std::collections::HashSet;
use std::io::Cursor;

use mp4parse::{
    read_mp4, CodecType, MediaContext, SampleEntry, Track, TrackType, VideoCodecSpecific,
};
#[cfg(not(target_arch = "arm"))]
use rav1d_safe::{Decoder as Av1Decoder, PixelLayout, Settings as Av1Settings};
use rust_h264::decoder::Decoder as H264Decoder;
use rust_h264::nal::{parse_avcc, parse_avcc_config};

use crate::ai_media::analyze_pixel_buffer;
use crate::image_nn::{self, ImageNnScores};

/// Maximum number of sampled frames per video (bounded decode cost).
pub const MAX_VIDEO_FRAMES: usize = 16;
/// Maximum pixel dimension of a decoded frame we will classify.
pub const MAX_FRAME_DIM: usize = 4096;
/// Maximum number of track samples we will materialize.
pub const MAX_TRACK_SAMPLES: usize = 1_000_000;
/// Maximum individual sample payload.
pub const MAX_SAMPLE_BYTES: usize = 64 * 1024 * 1024;
/// Long edge of the YUV→RGB downscale target (chrominance + NN input).
pub const MAX_SCALE_DIM: usize = 384;
/// rav1d frame size limit (pixels) — rejects hostile giant frames up front.
#[cfg(not(target_arch = "arm"))]
const AV1_FRAME_PIXELS: u32 = (MAX_FRAME_DIM * MAX_FRAME_DIM) as u32;

#[derive(Debug, Clone, Copy, PartialEq)]
enum VideoCodec {
    H264,
    #[cfg(not(target_arch = "arm"))]
    Av1,
}

/// Per-sample byte range + sync flag materialized from the MP4 tables.
#[derive(Debug, Clone, Copy)]
struct Mp4Sample {
    offset: usize,
    size: usize,
    is_sync: bool,
}

/// A decoded YUV frame (8-bit) with stride == width (no padding).
struct DecodedYuv {
    yw: usize,
    yh: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    /// Chroma plane dimensions (half-res for 4:2:0; full-res for 4:4:4).
    uw: usize,
    uh: usize,
    has_chroma: bool,
}

impl DecodedYuv {
    fn from_h264(f: rust_h264::decoder::Frame) -> Self {
        let (w, h) = (f.width as usize, f.height as usize);
        Self {
            yw: w,
            yh: h,
            y: f.y,
            u: f.u,
            v: f.v,
            uw: w / 2,
            uh: h / 2,
            has_chroma: true,
        }
    }

    #[cfg(not(target_arch = "arm"))]
    fn from_av1(f: &rav1d_safe::Frame) -> Option<Self> {
        if f.bit_depth() != 8 {
            return None; // 10/12-bit not supported by the hand-rolled RGB path
        }
        let (w, h) = (f.width() as usize, f.height() as usize);
        match f.pixel_layout() {
            PixelLayout::I420 | PixelLayout::I444 => {
                let chroma_full = matches!(f.pixel_layout(), PixelLayout::I444);
                let (uw, uh) = if chroma_full { (w, h) } else { (w / 2, h / 2) };
                let mut y = vec![0u8; w * h];
                let mut u = vec![0u8; uw * uh];
                let mut v = vec![0u8; uw * uh];
                copy_plane8(&mut y, w, h, 0, f);
                copy_plane8(&mut u, uw, uh, 1, f);
                copy_plane8(&mut v, uw, uh, 2, f);
                Some(Self {
                    yw: w,
                    yh: h,
                    y,
                    u,
                    v,
                    uw,
                    uh,
                    has_chroma: true,
                })
            }
            _ => None,
        }
    }
}

/// Copies a decoded 8-bit plane into a tight row-major Vec via `row()`.
#[cfg(not(target_arch = "arm"))]
fn copy_plane8(dst: &mut [u8], w: usize, h: usize, idx: usize, f: &rav1d_safe::Frame) {
    use rav1d_safe::Planes;
    if let Planes::Depth8(p) = f.planes() {
        let view = match idx {
            0 => Some(p.y()),
            1 => p.u(),
            2 => p.v(),
            _ => None,
        };
        if let Some(view) = view {
            let row_w = w.min(view.width());
            let row_h = h.min(view.height());
            let mut off = 0usize;
            for row in 0..row_h {
                let src = view.row(row);
                dst[off..off + row_w].copy_from_slice(&src[..row_w]);
                off += row_w;
            }
        }
    }
}

/// Per-frame signals from one classified video frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStat {
    /// Skin-tone chrominance exposure ratio.
    pub exposure: f32,
    /// Gore/blood chrominance ratio.
    pub gore_chrom: f32,
    /// Trained image NN head scores (None while the model is the untrained
    /// placeholder — the video layer then degrades to chrominance only).
    pub nn: Option<ImageNnScores>,
}

/// Aggregated per-frame NN + chrominance results for a whole video.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoFramesVerdict {
    /// Number of frames selected for sampling (attempted decodes).
    pub frames_attempted: usize,
    /// Number of frames that actually decoded + classified.
    pub frames_classified: usize,
    /// Max skin-exposure ratio across decoded frames.
    pub exposure_max: f32,
    /// Max gore-chrominance ratio across decoded frames.
    pub gore_chrom_max: f32,
    /// Max NN gore head score.
    pub gore_nn_max: f32,
    /// Max NN nudity head score.
    pub nudity_max: f32,
    /// Max NN juvenile head score.
    pub juvenile_max: f32,
    /// True when any decoded frame's NN gore verdict fired.
    pub any_nn_gore: bool,
    /// Count of frames whose composed NN CSAM-risk fired (report gate).
    pub csam_risk_frames: usize,
    /// Highest composed CSAM-risk score across frames.
    pub csam_risk_score_max: f32,
}

impl VideoFramesVerdict {
    /// Merges per-frame stats into the aggregate verdict. Pure data in/out —
    /// unit-testable without real decoders or weights.
    pub fn aggregate(stats: &[FrameStat], frames_attempted: usize) -> Self {
        let mut out = Self {
            frames_attempted,
            frames_classified: stats.len(),
            exposure_max: 0.0,
            gore_chrom_max: 0.0,
            gore_nn_max: 0.0,
            nudity_max: 0.0,
            juvenile_max: 0.0,
            any_nn_gore: false,
            csam_risk_frames: 0,
            csam_risk_score_max: 0.0,
        };
        for s in stats {
            out.exposure_max = out.exposure_max.max(s.exposure);
            out.gore_chrom_max = out.gore_chrom_max.max(s.gore_chrom);
            if let Some(nn) = s.nn {
                out.gore_nn_max = out.gore_nn_max.max(nn.gore);
                out.nudity_max = out.nudity_max.max(nn.nudity);
                out.juvenile_max = out.juvenile_max.max(nn.juvenile);
                let v = image_nn::compose_verdict(nn);
                if v.is_gore {
                    out.any_nn_gore = true;
                }
                if v.csam_risk {
                    out.csam_risk_frames += 1;
                    out.csam_risk_score_max = out.csam_risk_score_max.max(v.csam_risk_score);
                }
            }
        }
        out
    }
}

/// Materializes the MP4 sample table from stsz/stsc/stco/stss.
///
/// Offsets assume samples are stored contiguously within each chunk (the
/// universal encoder behaviour for H.264/AV1 video tracks).
fn build_sample_table(track: &Track) -> Option<Vec<Mp4Sample>> {
    let stsz = track.stsz.as_ref()?;
    let stsc = track.stsc.as_ref()?;
    let stco = track.stco.as_ref()?;
    let n_chunks = stco.offsets.len();
    if n_chunks == 0 {
        return None;
    }

    // Each chunk's sample count from the stsc run-length table.
    let mut chunk_samples = Vec::with_capacity(n_chunks);
    for c in 0..n_chunks {
        let mut spc = 0u32;
        for run in stsc.samples.iter() {
            if (run.first_chunk as usize) <= c + 1 {
                spc = run.samples_per_chunk;
            } else {
                break;
            }
        }
        if spc == 0 {
            return None;
        }
        chunk_samples.push(spc as usize);
    }
    let total: usize = chunk_samples.iter().sum();
    if total == 0 || total > MAX_TRACK_SAMPLES {
        return None;
    }

    let uniform = stsz.sample_size != 0;
    if uniform && stsz.sample_size as usize > MAX_SAMPLE_BYTES {
        return None;
    }
    let sizes: Vec<u32> = if uniform {
        vec![stsz.sample_size; total]
    } else {
        stsz.sample_sizes.to_vec()
    };
    if sizes.len() != total {
        return None;
    }

    let sync: HashSet<u32> = track
        .stss
        .as_ref()
        .map(|s| s.samples.iter().copied().collect())
        .unwrap_or_default();

    let mut samples = Vec::with_capacity(total);
    let mut si = 0usize;
    for (chunk_idx, &spc) in chunk_samples.iter().enumerate() {
        let mut off = stco.offsets[chunk_idx];
        for _ in 0..spc {
            if si >= total {
                break;
            }
            let sz = sizes[si] as u64;
            let end = off.checked_add(sz)?;
            if end > usize::MAX as u64 {
                return None;
            }
            samples.push(Mp4Sample {
                offset: off as usize,
                size: sz as usize,
                is_sync: false,
            });
            off = end;
            si += 1;
        }
    }
    for (i, s) in samples.iter_mut().enumerate() {
        s.is_sync = sync.contains(&((i + 1) as u32));
    }
    Some(samples)
}

/// Selects ≤ MAX_VIDEO_FRAMES sample indices, preferring evenly-spaced sync
/// samples (independently decodable keyframes, no reference-chain traversal).
fn select_samples(samples: &[Mp4Sample]) -> Vec<&Mp4Sample> {
    let sync: Vec<usize> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_sync)
        .map(|(i, _)| i)
        .collect();
    let pool: Vec<usize> = if sync.is_empty() {
        // No sync table: fall back to evenly-spaced arbitrary samples.
        (0..samples.len()).collect()
    } else {
        sync
    };
    let take = pool.len().min(MAX_VIDEO_FRAMES);
    let mut chosen = Vec::with_capacity(take);
    for k in 0..take {
        let idx = pool[(k * pool.len()) / take];
        chosen.push(&samples[idx]);
    }
    chosen
}

/// Decodes one H.264 sample (AVCC framing) into YUV. Fresh decoder per
/// sample: each chosen sample is a keyframe, so no reference chain needed.
fn decode_h264_sample(avcc: &[u8], sample: &[u8]) -> Option<DecodedYuv> {
    let config = parse_avcc_config(avcc).ok()?;
    let mut dec = H264Decoder::new();
    for nal in &config.sps_nals {
        let _ = dec.decode_nal(nal).ok();
    }
    for nal in &config.pps_nals {
        let _ = dec.decode_nal(nal).ok();
    }
    let nals = parse_avcc(sample, config.length_size);
    let mut out: Option<rust_h264::decoder::Frame> = None;
    for nal in &nals {
        if let Ok(Some(f)) = dec.decode_nal(nal) {
            out = Some(f);
        }
    }
    if let Some(f) = dec.flush() {
        out = Some(f);
    }
    out.map(DecodedYuv::from_h264)
}

/// Decodes one AV1 sample (raw OBU data preceded by the av1C config OBUs).
#[cfg(not(target_arch = "arm"))]
fn decode_av1_sample(config_obus: &[u8], sample: &[u8]) -> Option<DecodedYuv> {
    let mut settings = Av1Settings::default();
    settings.threads = 1; // synchronous deterministic decode per frame
    settings.frame_size_limit = AV1_FRAME_PIXELS;
    settings.apply_grain = false; // grain synthesis not needed for moderation
    let mut dec = Av1Decoder::with_settings(settings).ok()?;
    // threads=1 → decode() returns the completed frame synchronously.
    let mut frame: Option<rav1d_safe::Frame> = dec.decode(config_obus).ok().flatten();
    if frame.is_none() {
        frame = dec.decode(sample).ok().flatten();
    }
    // Fallback drain for decoders configured multi-threaded (paranoia).
    if frame.is_none() {
        frame = dec.get_frame().ok().flatten();
    }
    let f = frame?;
    match f.pixel_layout() {
        PixelLayout::I420 | PixelLayout::I444 => DecodedYuv::from_av1(&f),
        _ => None,
    }
}

/// Fits (w,h) into a ≤ MAX_SCALE_DIM box, preserving aspect ratio.
fn fit_scale(w: usize, h: usize) -> (usize, usize) {
    let scale = MAX_SCALE_DIM as f64 / w.max(h) as f64;
    if scale >= 1.0 {
        (w, h)
    } else {
        (
            ((w as f64) * scale).max(1.0) as usize,
            ((h as f64) * scale).max(1.0) as usize,
        )
    }
}

/// YUV (4:2:0 / 4:4:4 / 4:0:0, 8-bit) → interleaved RGB downscaled to the
/// target box via nearest-neighbour sampling. BT.601 conversion constants.
fn yuv_to_rgb(yuv: &DecodedYuv, tw: usize, th: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; tw * th * 3];
    let (uw, uh) = (yuv.uw.max(1), yuv.uh.max(1));
    for ty in 0..th {
        let sy = (ty * yuv.yh) / th;
        let cy = (sy * uh) / yuv.yh;
        for tx in 0..tw {
            let sx = (tx * yuv.yw) / tw;
            let cx = (sx * uw) / yuv.yw;
            let y = yuv.y[sy * yuv.yw + sx] as f32;
            let (u, v) = if yuv.has_chroma {
                let ci = cy * uw + cx;
                (yuv.u[ci] as f32, yuv.v[ci] as f32)
            } else {
                (128.0, 128.0)
            };
            let r = (y + 1.402 * (v - 128.0)).clamp(0.0, 255.0) as u8;
            let g = (y - 0.344_136 * (u - 128.0) - 0.714_136 * (v - 128.0)).clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * (u - 128.0)).clamp(0.0, 255.0) as u8;
            let o = (ty * tw + tx) * 3;
            rgb[o] = r;
            rgb[o + 1] = g;
            rgb[o + 2] = b;
        }
    }
    rgb
}

/// Runs chrominance + image NN on a decoded frame.
fn classify_frame(yuv: &DecodedYuv) -> Option<FrameStat> {
    if yuv.yw == 0 || yuv.yh == 0 || yuv.yw > MAX_FRAME_DIM || yuv.yh > MAX_FRAME_DIM {
        return None;
    }
    let (tw, th) = fit_scale(yuv.yw, yuv.yh);
    let rgb = yuv_to_rgb(yuv, tw, th);
    let (exposure, gore_chrom) = analyze_pixel_buffer(&rgb, 3, tw, th);
    let img = image::RgbImage::from_raw(tw as u32, th as u32, rgb)?;
    let nn = image_nn::classify_rgb(&img);
    Some(FrameStat {
        exposure,
        gore_chrom,
        nn,
    })
}

/// Classifies an MP4 video buffer by running the image NN (+ chrominance)
/// against sampled frames. `None` when the buffer isn't a supported MP4 with
/// an H.264/AV1 video track, or when no sampled frame decoded.
pub fn classify_video_mp4(bytes: &[u8]) -> Option<VideoFramesVerdict> {
    let ctx: MediaContext = read_mp4(&mut Cursor::new(bytes)).ok()?;
    let track = ctx
        .tracks
        .iter()
        .find(|t| t.track_type == TrackType::Video)?;
    let stsd = track.stsd.as_ref()?;
    let entry = stsd.descriptions.iter().find_map(|d| match d {
        SampleEntry::Video(v) => Some(v),
        _ => None,
    })?;

    let (codec, config) = match &entry.codec_specific {
        VideoCodecSpecific::AVCConfig(avcc) => (VideoCodec::H264, avcc.to_vec()),
        #[cfg(not(target_arch = "arm"))]
        VideoCodecSpecific::AV1Config(cfg) => (VideoCodec::Av1, cfg.config_obus().to_vec()),
        _ => return None, // HEVC/VP8/VP9 / (on armv7) AV1 — caller falls back
    };
    let supported = entry.codec_type == CodecType::H264
        || (entry.codec_type == CodecType::AV1 && cfg!(not(target_arch = "arm")));
    if !supported {
        return None;
    }

    let samples = build_sample_table(track)?;
    if samples.is_empty() {
        return None;
    }
    let chosen = select_samples(&samples);
    let attempted = chosen.len();
    let mut stats = Vec::new();
    for s in &chosen {
        let start = s.offset as u64;
        let end = start.checked_add(s.size as u64)?;
        if end > bytes.len() as u64 {
            continue; // corrupt table: sample runs past EOF
        }
        let slice = &bytes[start as usize..end as usize];
        if slice.len() > MAX_SAMPLE_BYTES {
            continue;
        }
        let yuv = match codec {
            VideoCodec::H264 => decode_h264_sample(&config, slice),
            #[cfg(not(target_arch = "arm"))]
            VideoCodec::Av1 => decode_av1_sample(&config, slice),
        };
        if let Some(yuv) = yuv {
            if let Some(stat) = classify_frame(&yuv) {
                stats.push(stat);
            }
        }
    }
    if stats.is_empty() {
        return None;
    }
    Some(VideoFramesVerdict::aggregate(&stats, attempted))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H264_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/h264_64x64.mp4");
    const AV1_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/av1_64x64.mp4");

    fn stat(exposure: f32, gore: f32, nn: Option<(f32, f32, f32)>) -> FrameStat {
        FrameStat {
            exposure,
            gore_chrom: gore,
            nn: nn.map(|(g, n, j)| ImageNnScores {
                gore: g,
                nudity: n,
                juvenile: j,
            }),
        }
    }

    // ---------- aggregation rules (mirror ai_media `finalize_video_verdict`) --

    #[test]
    fn aggregate_adult_nudity_alone_never_blocks() {
        let v = VideoFramesVerdict::aggregate(
            &[stat(0.1, 0.1, None), stat(0.1, 0.1, Some((0.1, 0.98, 0.1)))],
            2,
        );
        assert!(v.nudity_max >= 0.98);
        assert!(!v.any_nn_gore);
        assert_eq!(
            v.csam_risk_frames, 0,
            "adult nudity must not raise CSAM risk"
        );
    }

    #[test]
    fn aggregate_nn_gore_sets_flag() {
        let v = VideoFramesVerdict::aggregate(
            &[stat(0.1, 0.1, Some((0.96, 0.1, 0.1))), stat(0.2, 0.3, None)],
            2,
        );
        assert!(v.any_nn_gore);
        assert!(v.gore_nn_max >= 0.96);
        assert!(v.gore_chrom_max >= 0.3);
    }

    #[test]
    fn aggregate_csam_risk_counts_frames_and_max() {
        let v = VideoFramesVerdict::aggregate(
            &[
                stat(0.1, 0.1, Some((0.2, 0.95, 0.9))),
                stat(0.1, 0.1, Some((0.2, 0.9, 0.85))),
                stat(0.1, 0.1, Some((0.2, 0.1, 0.1))),
            ],
            3,
        );
        assert_eq!(v.csam_risk_frames, 2);
        assert!(v.csam_risk_score_max > 0.5);
    }

    #[test]
    fn aggregate_no_nn_degrades_to_chrominance() {
        let v = VideoFramesVerdict::aggregate(&[stat(0.55, 0.12, None), stat(0.1, 0.02, None)], 2);
        assert_eq!(v.frames_classified, 2);
        assert_eq!(v.gore_nn_max, 0.0);
        assert!(v.exposure_max >= 0.55);
        assert!(!v.any_nn_gore);
    }

    // ---------- sample table + selection --------------------------------

    #[allow(clippy::field_reassign_with_default)]
    fn fake_track() -> Track {
        use mp4parse::{
            SampleSizeBox, SampleToChunk, SampleToChunkBox, SyncSampleBox, TimeToSampleBox,
        };
        let mut t = Track::default();
        t.track_type = TrackType::Video;
        // 2 chunks: [0,100), [100,115); sample sizes 10,10,5; sync = sample 1.
        t.stsz = Some(SampleSizeBox {
            sample_size: 0,
            sample_sizes: vec![10, 10, 5].into(),
        });
        t.stsc = Some(SampleToChunkBox {
            samples: vec![
                SampleToChunk {
                    first_chunk: 1,
                    samples_per_chunk: 2,
                    sample_description_index: 1,
                },
                SampleToChunk {
                    first_chunk: 2,
                    samples_per_chunk: 1,
                    sample_description_index: 1,
                },
            ]
            .into(),
        });
        t.stco = Some(mp4parse::ChunkOffsetBox {
            offsets: vec![0, 100].into(),
        });
        t.stss = Some(SyncSampleBox {
            samples: vec![1].into(),
        });
        t.stts = Some(TimeToSampleBox {
            samples: vec![mp4parse::Sample {
                sample_count: 3,
                sample_delta: 1,
            }]
            .into(),
        });
        t
    }

    #[test]
    fn sample_table_offsets_and_sync() {
        let samples = build_sample_table(&fake_track()).unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].offset, 0);
        assert_eq!(samples[0].size, 10);
        assert!(samples[0].is_sync);
        assert_eq!(samples[1].offset, 10);
        assert_eq!(samples[1].size, 10);
        assert!(!samples[1].is_sync);
        assert_eq!(samples[2].offset, 100);
        assert_eq!(samples[2].size, 5);
        assert!(!samples[2].is_sync);
    }

    #[test]
    fn sync_pool_selected_evenly() {
        let samples: Vec<Mp4Sample> = (0..20_usize)
            .map(|i| Mp4Sample {
                offset: i * 10,
                size: 10,
                is_sync: true,
            })
            .collect();
        let chosen = select_samples(&samples);
        assert!(chosen.len() <= MAX_VIDEO_FRAMES);
        assert_eq!(chosen.len(), 16);
        // First sync sample sampled; spread covers the tail.
        assert_eq!(chosen[0].offset, 0);
        assert!(chosen.last().unwrap().offset >= 160);
    }

    #[test]
    fn non_sync_pool_falls_back_to_even_spacing() {
        let samples: Vec<Mp4Sample> = (0..4_usize)
            .map(|i| Mp4Sample {
                offset: i * 10,
                size: 10,
                is_sync: false,
            })
            .collect();
        let chosen = select_samples(&samples);
        assert_eq!(chosen.len(), 4);
    }

    // ---------- YUV → RGB ----------------------------------------------

    #[test]
    fn yuv_red_and_gray_checkpoints() {
        // 2x2 I420; pure red (y=76, u=84, v=255).
        let red = DecodedYuv {
            yw: 2,
            yh: 2,
            y: vec![76, 76, 76, 76],
            u: vec![84, 84],
            v: vec![255, 255],
            uw: 1,
            uh: 1,
            has_chroma: true,
        };
        let rgb = yuv_to_rgb(&red, 2, 2);
        assert!(rgb[0] as u16 > rgb[1] as u16, "r={} g={}", rgb[0], rgb[1]);
        assert!(rgb[0] as u16 > rgb[2] as u16);
        assert!(rgb[1] < 60, "green too high: {}", rgb[1]);

        // Mid gray (y=128, u=128, v=128) → all 128.
        let gray = DecodedYuv {
            yw: 2,
            yh: 2,
            y: vec![128, 128, 128, 128],
            u: vec![128, 128],
            v: vec![128, 128],
            uw: 1,
            uh: 1,
            has_chroma: true,
        };
        let rgb = yuv_to_rgb(&gray, 2, 2);
        assert!(rgb[1] == 128 && rgb[0] == 128 && rgb[2] == 128, "{rgb:?}");
    }

    #[test]
    fn fit_scale_preserves_aspect() {
        assert_eq!(fit_scale(64, 64), (64, 64));
        let (w, h) = fit_scale(1920, 1080);
        assert!(w <= 384 && h <= 384);
        assert_eq!(w * 1080, h * 1920); // same aspect
    }

    // ---------- full-file pipeline on generated fixtures ----------------

    #[test]
    fn h264_mp4_classifies_frames() {
        let v = classify_video_mp4(H264_FIXTURE);
        let v = v.expect("h264 fixture should classify");
        assert!(v.frames_classified >= 1, "{v:?}");
        assert!(v.exposure_max >= 0.0 && v.exposure_max <= 1.0);
    }

    #[test]
    #[cfg(not(target_arch = "arm"))]
    fn av1_mp4_classifies_frames() {
        let v = classify_video_mp4(AV1_FIXTURE);
        let v = v.expect("av1 fixture should classify");
        assert!(v.frames_classified >= 1, "{v:?}");
    }

    #[test]
    fn non_video_bytes_return_none() {
        assert!(classify_video_mp4(b"not an mp4 at all").is_none());
        // Still-image bytes (JPEG) are not video.
        let mut jpg = Vec::new();
        image::DynamicImage::new_rgb8(16, 16)
            .write_to(
                &mut std::io::Cursor::new(&mut jpg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        assert!(classify_video_mp4(&jpg).is_none());
    }

    #[test]
    fn truncated_mp4_returns_none_without_panic() {
        let cut = &H264_FIXTURE[..H264_FIXTURE.len() / 2];
        let _ = classify_video_mp4(cut); // must not panic
    }
}
