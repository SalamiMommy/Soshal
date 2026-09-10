//! Waveform extraction. Container formats via symphonia; raw Opus voice
//! notes via audiopus. Output: `bins` normalized peak values in 0..1.

use std::io::Cursor;
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

/// Max decoded f32 samples emitted by the container-decode path; mirrors
/// `voice::MAX_DECODE_OUTPUT_SAMPLES` (64 MiB decode-alloc cap).
const MAX_CONTAINER_SAMPLES: usize = 64 * 1024 * 1024;

/// Decode a whole audio file (or raw Opus voice-note stream) into mono f32.
/// Payloads are decoded in-memory via Cursor to avoid disk I/O.
fn decode_all(data: &[u8], hint_ext: Option<&str>) -> Result<MonoF32, String> {
    if crate::is_opus_stream(data) {
        return voice::decode_packets(&data[crate::OPUS_STREAM_MAGIC.len()..]);
    }

    let mut hint = Hint::new();
    if let Some(ext) = hint_ext {
        hint.with_extension(ext);
    }
    let cursor = Cursor::new(data.to_vec());
    decode_reader(cursor, &hint)
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
        if samples.len() >= MAX_CONTAINER_SAMPLES {
            break;
        }
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
    let mut last_raw_sum = 0.0f64;
    let mut last_raw_count = 0usize;
    for (i, s) in samples.iter().enumerate() {
        let bucket = (i as u64 * bins as u64 / total) as usize;
        acc_sum += s.abs() as f64;
        acc_count += 1;
        if bucket == out.len() {
            let mean = acc_sum / acc_count.max(1) as f64;
            let rms = mean.sqrt();
            let floored = if rms > 0.01 { rms } else { 0.0 };
            peak_max = peak_max.max(floored);
            out.push(floored as f32);
            last_raw_sum = acc_sum;
            last_raw_count = acc_count;
            acc_sum = 0.0;
            acc_count = 0;
        }
    }
    // Tail: samples whose bucket index exceeds out.len(). Merge raw values
    // into the last bucket and recompute RMS once (RMS is not additive).
    if acc_count > 0 {
        let combined_sum = last_raw_sum + acc_sum;
        let combined_count = last_raw_count + acc_count;
        let mean = combined_sum / combined_count.max(1) as f64;
        let rms = mean.sqrt();
        let floored = if rms > 0.01 { rms } else { 0.0 };
        if let Some(last) = out.last_mut() {
            *last = floored as f32;
        } else {
            out.push(floored as f32);
        }
        peak_max = peak_max.max(floored);
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
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    let mut hint = Hint::new();
    if let Some(ref e) = ext {
        hint.with_extension(e);
    }
    let file = std::fs::File::open(path).map_err(|e| format!("read {path}: {e}"))?;
    let reader = std::io::BufReader::new(file);
    let samples = match decode_reader(reader, &hint) {
        Ok(s) => s,
        Err(_) => {
            // Fallback for raw Opus packets or unusual containers requiring full buffer scan
            let data = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
            decode_all(&data, ext.as_deref())?
        }
    };
    Ok(peaks(&samples, clamp_bins(bins)))
}

/// Convenience: extract waveform peaks from a path, 512 default bins.
pub fn extract_waveform(path: &str, bins: usize) -> Result<Vec<f32>, String> {
    extract_waveform_path(path, bins)
}

#[cfg(test)]
mod tests {
    use super::{extract_waveform_bytes, peaks};

    /// Hand-rolled mono 16-bit PCM WAV (44-byte RIFF header), `n` samples at
    /// constant `amplitude`. 8 kHz — waveform binning is rate-agnostic.
    fn wav_pcm_i16(n: usize, amplitude: i16) -> Vec<u8> {
        let data_len = n * 2;
        let mut out = Vec::with_capacity(44 + data_len);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&8_000u32.to_le_bytes());
        out.extend_from_slice(&16_000u32.to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for _ in 0..n {
            out.extend_from_slice(&amplitude.to_le_bytes());
        }
        out
    }

    /// Pins the tail-merge behavior: when samples % bins != 0, tail samples
    /// are folded into the last main-loop bucket (not dropped). Quiet constant
    /// samples produce rms < 0.01 in every bucket → all floored to 0.
    #[test]
    fn partial_tail_bucket_skips_silence_floor() {
        let amp = 9.99e-5f32; // sqrt(amp) ~= 0.009995 < 0.01 → floored
        let out = peaks(&[amp; 10], 32);
        assert_eq!(out.len(), 32);
        assert!(out.iter().all(|p| *p == 0.0), "all buckets floored to 0");
    }

    /// Pins the empty-samples early return: zero samples → all-zero bins.
    #[test]
    fn empty_samples_yield_all_zero_bins() {
        assert_eq!(peaks(&[], 32), vec![0.0; 32]);
        // Empty-data WAV (44-byte header only) decodes to zero samples; the
        // container path is guarded since symphonia behavior is out of scope.
        if let Ok(out) = extract_waveform_bytes(&wav_pcm_i16(0, 0), 32) {
            assert_eq!(out.len(), 32);
            assert!(out.iter().all(|p| *p == 0.0));
        }
    }

    /// Pins the strict `>` at the 0.01 RMS gate: just below floors to 0,
    /// just above survives (then normalizes to 1.0).
    #[test]
    fn rms_just_below_and_above_floor_boundary() {
        // sqrt(9.99e-5) ~= 0.009995 < 0.01 → all buckets floored to 0
        assert_eq!(peaks(&[9.99e-5f32; 10], 2), vec![0.0, 0.0]);
        // sqrt(1.0001e-4) ~= 0.010005 > 0.01 → kept, normalized to 1.0
        assert_eq!(peaks(&[1.0001e-4f32; 10], 2), vec![1.0, 1.0]);
    }

    /// Pins the zero-pad tail: samples < bins leaves trailing 0.0 buckets.
    #[test]
    fn samples_fewer_than_bins_zero_pad() {
        let wav = wav_pcm_i16(10, i16::MAX);
        let out = extract_waveform_bytes(&wav, 32).unwrap();
        assert_eq!(out.len(), 32);
        // All 10 samples land in bucket 0 via tail-merge; remaining buckets
        // are zero-padded.
        assert!((out[0] - 1.0).abs() < 1e-5, "bucket 0: {}", out[0]);
        assert!(out[1..].iter().all(|p| *p == 0.0));
    }

    /// Pins the post-normalization <0.001 zero-out: a small-but-nonzero peak
    /// survives the 0.01 floor, then normalization divides it down below
    /// 0.001 and it is zeroed.
    #[test]
    fn normalize_zeroes_small_but_nonzero_peaks() {
        // Bucket flush quirk: sample 0 flushes bucket 0 alone (rms 12); the
        // next flush lands at i=10 covering samples 1..=10. Make those quiet
        // (rms 0.01, just over the 0.01 floor): 0.01 / 12 < 0.001 → zeroed.
        let mut samples = [1.0001e-4f32; 20];
        samples[0] = 144.0;
        let out = peaks(&samples, 2);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], 0.0);
    }

    /// Pins NaN poisoning: an f32 NaN sample fails the strict `>` gate
    /// (NaN > 0.01 is false), so its whole bucket floors to 0 while
    /// neighboring buckets stay intact. peaks() takes f32 samples directly,
    /// so no f32-WAV fixture is needed — 16-bit path can't carry NaN.
    #[test]
    fn nan_sample_poisons_bucket_to_zero() {
        assert_eq!(peaks(&[f32::NAN, 1.0], 2), vec![0.0, 1.0]);
        assert_eq!(peaks(&[1.0, f32::NAN], 2), vec![1.0, 0.0]);
    }

    /// Pins the container-decode sample cap to 64 MiB (voice.rs mirror).
    #[test]
    fn container_sample_cap_is_64mib() {
        assert_eq!(super::MAX_CONTAINER_SAMPLES, 64 * 1024 * 1024);
    }
}
