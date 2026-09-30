//! Benchmarks for the text moderation path (`check::check_with_custom_words`).
//!
//! Purpose: quantify the per-row cost that a feed page fetch pays, since the
//! per-row moderation call over a fetched page is the target of the batched +
//! parallel change in `flutter-bridge/src/ffi/feed.rs`. The feed reads pages of
//! up to 800 candidate rows and calls this once per row, so the number that
//! matters is (a) the cost of one call on a realistic post body and (b) the cost
//! of a whole page's worth of calls, serial vs parallel — (b) is exactly the
//! budget the change removes.
//!
//! Bodies are synthetic and self-contained (no committed corpora, no fixture
//! files). Length is varied because `check_text_comprehensive` short-circuits
//! on a hit, so a single short string would measure only the fast path.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use rayon::prelude::*;
use soshal_content_core::compress::{compress_json_dict, decompress_json_dict};
use soshal_moderation_core::check::check_with_custom_words;

/// Rows a feed page can hold before moderation filtering runs.
const MAX_PAGE_ROWS: usize = 800;

/// A post body long enough to exercise the full pipeline — CSAM keyword scan,
/// gore patterns, normalized variants, the regex set, the spam heuristics and
/// the text NN — without tripping any of them. Real feed posts are this shape:
/// ordinary prose plus a hashtag tail.
fn body(i: usize, len: usize) -> String {
    const SEED: &[&str] = &[
        "spent the morning tuning the relay sync window and it finally stopped",
        "the whole point of a federated timeline is that nobody gets to curate",
        "anyone else find that offline-first drafts are the single best thing",
        "walking the ridge at sunrise with nothing but a paper map and no signal",
        "rewrote the indexer to stream rows instead of buffering the whole page",
        "the trick with content-defined chunking is the rolling window, not the hash",
    ];
    let mut s = String::with_capacity(len + 32);
    s.push_str(&format!("post {} ", i));
    let mut n = 0usize;
    while s.len() < len {
        s.push_str(SEED[(i + n) % SEED.len()]);
        s.push(' ');
        n += 1;
    }
    s.push_str("#soshal #fediverse");
    s
}

fn bodies(count: usize, len: usize) -> Vec<String> {
    (0..count).map(|i| body(i, len)).collect()
}

/// One call, per body length. This is the number the row filter multiplies by.
fn bench_single(c: &mut Criterion) {
    let no_filters: Vec<String> = Vec::new();
    let mut group = c.benchmark_group("text_check_single");
    for len in [80usize, 320, 1200] {
        let s = body(0, len);
        group.throughput(Throughput::Bytes(s.len() as u64));
        group.bench_function(format!("{len}B"), |b| {
            b.iter_batched(
                || s.as_str(),
                |t| check_with_custom_words(t, &no_filters),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

/// A whole page's moderation pass, serial vs parallel. This is the budget the
/// `feed.rs` change removes. `serial` is the reference; the two should diverge
/// by roughly the core count once the batch is big enough to amortize dispatch.
fn bench_page_budget(c: &mut Criterion) {
    let rows = bodies(MAX_PAGE_ROWS, 320);
    let no_filters: Vec<String> = Vec::new();
    let mut group = c.benchmark_group("text_check_page");
    group.throughput(Throughput::Elements(MAX_PAGE_ROWS as u64));
    group.bench_function("800x_320B_serial", |b| {
        b.iter(|| {
            let mut hits = 0usize;
            for r in &rows {
                if !check_with_custom_words(r, &no_filters).passed {
                    hits += 1;
                }
            }
            std::hint::black_box(hits)
        })
    });
    group.bench_function("800x_320B_parallel", |b| {
        b.iter(|| {
            let hits = rows
                .par_iter()
                .filter(|r| !check_with_custom_words(r, &no_filters).passed)
                .count();
            std::hint::black_box(hits)
        })
    });
    group.finish();
}

/// Window sizing. `feed.rs` classifies in windows so a mostly-clean page still
/// stops early; a window that is too small leaves cores idle and one that is
/// too large wastes classification on rows past the page limit. Both extremes
/// are benched so the chosen constant is not a guess.
fn bench_window(c: &mut Criterion) {
    let rows = bodies(MAX_PAGE_ROWS, 320);
    let no_filters: Vec<String> = Vec::new();
    let mut group = c.benchmark_group("text_check_window");
    for window in [16usize, 64, 256] {
        group.throughput(Throughput::Elements(window as u64));
        group.bench_function(format!("window{window}_parallel"), |b| {
            b.iter(|| {
                let mut hits = 0usize;
                for chunk in rows.chunks(window) {
                    hits += chunk
                        .par_iter()
                        .filter(|r| !check_with_custom_words(r, &no_filters).passed)
                        .count();
                }
                std::hint::black_box(hits)
            })
        });
    }
    group.finish();
}

/// The zstd half of the feed fetch, which the same change parallelizes.
/// Measured on a real dict-frame payload at the body sizes a rich post (long
/// text + media tags) reaches, worst case being every row compressed.
fn bench_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("feed_decompress");
    for len in [320usize, 1200, 4000] {
        let raw = body(0, len);
        let enc = compress_json_dict(&raw);
        assert_eq!(decompress_json_dict(&enc), raw, "fixture must round-trip");
        group.throughput(Throughput::Bytes(raw.len() as u64));
        group.bench_function(format!("{len}B_serial"), |b| {
            b.iter(|| {
                let mut n = 0usize;
                for _ in 0..MAX_PAGE_ROWS {
                    n += decompress_json_dict(std::hint::black_box(&enc)).len();
                }
                std::hint::black_box(n)
            })
        });
        group.bench_function(format!("{len}B_parallel"), |b| {
            b.iter(|| {
                let n = (0..MAX_PAGE_ROWS)
                    .into_par_iter()
                    .map(|_| decompress_json_dict(std::hint::black_box(&enc)).len())
                    .sum::<usize>();
                std::hint::black_box(n)
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_single,
    bench_page_budget,
    bench_window,
    bench_decompress
);
criterion_main!(benches);
