//! Benchmarks for the video moderation path (`video_nn`).
//!
//! Purpose: quantify the per-frame cost that `classify_video_mp4` pays, since
//! the frame loop there is the target of the `par_iter` change. Two
//! measurements, deliberately fixture-free and fixture-backed:
//!
//! 1. `classify_video_mp4` end-to-end on the small committed fixtures — proves
//!    the demux → sample-select → decode → classify path stays healthy.
//! 2. `classify_rgb` at realistic frame resolutions — this is the per-frame
//!    cost that dominates at 1080p, and ×`MAX_VIDEO_FRAMES` is exactly the
//!    serial budget the parallel change removes.
//!
//! The 64×64 fixtures are too small to time meaningfully in absolute terms;
//! that is why the per-frame work is measured at 1080p/720p/384p instead of
//! committing multi-megabyte clips.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use rayon::prelude::*;
use soshal_moderation_core::image_nn;
use soshal_moderation_core::video_nn::classify_video_mp4;

const H264_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/h264_64x64.mp4");
const AV1_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/av1_64x64.mp4");

/// Frames `classify_video_mp4` samples per video.
const MAX_VIDEO_FRAMES: usize = 16;

/// Synthetic frame with enough structure that the conv layers do real work
/// (a flat buffer would let the SIMD paths short-circuit and under-report).
fn synthetic_rgb(w: u32, h: u32) -> image::RgbImage {
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 3];
    for (i, px) in buf.chunks_exact_mut(3).enumerate() {
        let x = i % w as usize;
        let y = i / w as usize;
        px[0] = ((x * 7 + y * 3) % 256) as u8;
        px[1] = ((x * 3) ^ (y * 5)) as u8;
        px[2] = ((x * x + y * y) % 251) as u8;
    }
    image::RgbImage::from_raw(w, h, buf).expect("synthetic frame fits its own dimensions")
}

fn bench_video_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("video_classify_mp4");
    group.throughput(Throughput::Bytes(H264_FIXTURE.len() as u64));
    // A 64×64 clip is ~1 ms end to end, so batch it: the sample here is the
    // path, not the absolute per-call latency.
    group.bench_function("h264_64x64", |b| {
        b.iter_batched(|| H264_FIXTURE, classify_video_mp4, BatchSize::SmallInput)
    });
    group.bench_function("av1_64x64", |b| {
        b.iter_batched(|| AV1_FIXTURE, classify_video_mp4, BatchSize::SmallInput)
    });
    group.finish();
}

/// Per-frame NN + chrominance cost at the resolutions real uploads arrive at.
/// `classify_video_mp4` scales every frame down to a 384px long edge before
/// this, so 1080p here represents decode-input scale, not NN-input scale.
fn bench_per_frame(c: &mut Criterion) {
    if !image_nn::image_nn_available() {
        eprintln!("skipping per-frame bench: image NN asset unavailable");
        return;
    }
    let mut group = c.benchmark_group("video_per_frame");
    for (label, dim) in [("1080p", 1920u32), ("720p", 1280), ("384p", 384)] {
        let frame = synthetic_rgb(dim, dim / 16 * 9);
        group.throughput(Throughput::Elements(1));
        group.bench_function(label, |b| b.iter(|| image_nn::classify_rgb(&frame)));
    }
    group.finish();
}

/// One full video's worth of per-frame work (16 frames at 1080p), the budget
/// `classify_video_mp4` spends. Both dispatch modes are benched so the
/// rayon change in `video_nn.rs` stays measurable: `serial` is the reference
/// the parallel path is judged against, and the two should diverge by roughly
/// the core count. Note these bypass `classify_video_mp4` deliberately — they
/// isolate per-frame CPU, whereas `video_classify_mp4` above measures the real
/// path including demux and decode.
fn bench_full_video_budget(c: &mut Criterion) {
    if !image_nn::image_nn_available() {
        eprintln!("skipping video budget bench: image NN asset unavailable");
        return;
    }
    let frame = synthetic_rgb(1920, 1080);
    let mut group = c.benchmark_group("video_full_budget");
    group.throughput(Throughput::Elements(MAX_VIDEO_FRAMES as u64));
    group.bench_function("16x_1080p_serial", |b| {
        b.iter(|| {
            for _ in 0..MAX_VIDEO_FRAMES {
                std::hint::black_box(image_nn::classify_rgb(std::hint::black_box(&frame)));
            }
        })
    });
    group.bench_function("16x_1080p_parallel", |b| {
        b.iter(|| {
            (0..MAX_VIDEO_FRAMES).into_par_iter().for_each(|_| {
                std::hint::black_box(image_nn::classify_rgb(std::hint::black_box(&frame)));
            });
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_video_path,
    bench_per_frame,
    bench_full_video_budget
);
criterion_main!(benches);
