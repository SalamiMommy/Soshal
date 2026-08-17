//! Waveform extraction. Container formats via symphonia; raw Opus voice
//! notes via audiopus. Output: `bins` normalized peak values in 0..1.

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::voice;
use crate::MonoF32;

pub const WAVEFORM_MIN_BINS: usize = 32;
pub const WAVEFORM_MAX_BINS: usize = 2048;

/// Decode a whole audio file (or raw Opus voice-note stream) into mono f32.
/// Byte payloads are spilled to a temp file so symphonia can seek (m4a etc).
fn decode_all(data: &[u8], hint_ext: Option<&str>) -> Result<MonoF32, String> {
    if crate::is_opus_stream(data) {
        return voice::decode_packets(&data[crate::OPUS_STREAM_MAGIC.len()..]);
    }

    let mut hint = Hint::new();
    if let Some(ext) = hint_ext {
        hint.with_extension(ext);
    }
    let tmp_path = temp_file_path();
    std::fs::write(&tmp_path, data).map_err(|e| format!("write temp: {e}"))?;
    let file = std::fs::File::open(&tmp_path).map_err(|e| format!("open temp: {e}"))?;
    let result = decode_reader(file, &hint);
    let _ = std::fs::remove_file(&tmp_path);
    result
}

fn temp_file_path() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "soshal-waveform-{}-{}-{seq}.bin",
        std::process::id(),
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace(['/', '\\'], "_")
    ))
}

fn decode_reader<R: std::io::Read + std::io::Seek + Send + Sync + 'static>(
    reader: R,
    hint: &Hint,
) -> Result<MonoF32, String> {
    use symphonia::core::io::MediaSource;
    struct FileSource<R: std::io::Read + std::io::Seek> {
        inner: R,
    }
    impl<R: std::io::Read + std::io::Seek> std::io::Read for FileSource<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buf)
        }
    }
    impl<R: std::io::Read + std::io::Seek> std::io::Seek for FileSource<R> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }
    impl<R: std::io::Read + std::io::Seek + Send + Sync> MediaSource for FileSource<R> {
        fn is_seekable(&self) -> bool {
            true
        }
        fn byte_len(&self) -> Option<u64> {
            None
        }
    }
    let mss = MediaSourceStream::new(Box::new(FileSource { inner: reader }), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe: {e}"))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "no default track".to_string())?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(
            &params,
            &DecoderOptions {
                ..Default::default()
            },
        )
        .map_err(|e| format!("codec init: {e}"))?;

    let channels = params.channels.map(|c| c.count()).unwrap_or(2).max(1);
    let sample_rate = params.sample_rate.unwrap_or(48_000);

    let mut samples: MonoF32 = Vec::new();
    let mut sb: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("decode: {e}"))?;
        append_mono(&mut samples, &decoded, &mut sb, channels, sample_rate);
    }
    Ok(samples)
}

fn append_mono(
    out: &mut MonoF32,
    buf: &symphonia::core::audio::AudioBufferRef<'_>,
    sb: &mut Option<SampleBuffer<f32>>,
    channels: usize,
    sample_rate: u32,
) {
    let needed = buf.capacity();
    let buf_ref = sb.get_or_insert_with(|| SampleBuffer::<f32>::new(needed as u64, *buf.spec()));
    if buf_ref.capacity() < needed {
        // Decoded frame grew (larger packet than any seen so far): grow the
        // reusable buffer once instead of allocating per packet.
        *buf_ref = SampleBuffer::<f32>::new((needed * 2) as u64, *buf.spec());
    }
    buf_ref.copy_interleaved_ref(buf.clone());
    let interleaved = buf_ref.samples();
    let ch = channels.max(1);
    if ch <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    for frame in interleaved.chunks(ch) {
        let sum: f32 = frame.iter().take(ch).sum::<f32>();
        out.push(sum / ch as f32);
    }
    // Callers that need timing use the native rate from decode_all; the
    // waveform binning below is rate-agnostic.
    let _ = sample_rate;
}

/// Collapse mono samples into `bins` RMS peaks, each normalized 0..1.
fn peaks(samples: &[f32], bins: usize) -> Vec<f32> {
    if samples.is_empty() {
        return vec![0.0; bins];
    }
    let mut out = Vec::with_capacity(bins);
    let total = samples.len() as u64;
    let mut peak_max = 0.0f64;
    let mut acc_sum = 0.0f64;
    let mut acc_count = 0usize;
    for (i, s) in samples.iter().enumerate() {
        // Integer bucket math (u64: safe on 32-bit targets too) instead of a
        // per-sample f64 division.
        let bucket = (i as u64 * bins as u64 / total) as usize;
        acc_sum += s.abs() as f64;
        acc_count += 1;
        if bucket == out.len() {
            let mean = acc_sum / acc_count.max(1) as f64;
            let rms = mean.sqrt();
            // Absolute silence gate: codec warm-up noise / near-silence must
            // read as 0. Real content RMS (speech~0.02-0.2) sits far above.
            let floored = if rms > 0.01 { rms } else { 0.0 };
            peak_max = peak_max.max(floored);
            out.push(floored as f32);
            acc_sum = 0.0;
            acc_count = 0;
        }
    }
    if acc_count > 0 {
        let mean = acc_sum / acc_count.max(1) as f64;
        out.push(mean.sqrt() as f32);
    }
    while out.len() < bins {
        out.push(0.0);
    }
    out.truncate(bins);
    if peak_max > 0.0 {
        for v in out.iter_mut() {
            *v = ((*v as f64 / peak_max).min(1.0)) as f32;
            if *v < 0.001 {
                *v = 0.0;
            }
        }
    }
    out
}

fn clamp_bins(bins: usize) -> usize {
    bins.clamp(WAVEFORM_MIN_BINS, WAVEFORM_MAX_BINS)
}

/// Extract waveform peaks from raw audio bytes. Container sniffed; raw Opus
/// voice-note envelopes routed to audiopus.
pub fn extract_waveform_bytes(data: &[u8], bins: usize) -> Result<Vec<f32>, String> {
    let samples = decode_all(data, None)?;
    Ok(peaks(&samples, clamp_bins(bins)))
}

/// Extract waveform peaks from a file on disk. `hint_ext` helps the prober
/// when the container is ambiguous (e.g. ".m4a").
pub fn extract_waveform_path(path: &str, bins: usize) -> Result<Vec<f32>, String> {
    let data = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    let samples = decode_all(&data, ext.as_deref())?;
    Ok(peaks(&samples, clamp_bins(bins)))
}

/// Convenience: extract waveform peaks from a path, 512 default bins.
pub fn extract_waveform(path: &str, bins: usize) -> Result<Vec<f32>, String> {
    extract_waveform_path(path, bins)
}
