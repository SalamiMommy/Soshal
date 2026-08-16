//! Integration tests for soshal-audio-core: Opus voice-note codec, framing
//! validation, waveform extraction (voice-note + WAV container), and helpers.
//!
//! Pure Rust — no devices, no FFI. Encoding/decoding paths only.

use soshal_audio_core::voice::{
    decode_voice_stream, encode_voice_pcm, pcm_len_for_secs, voice_duration_secs,
};
use soshal_audio_core::{
    extract_waveform, extract_waveform_bytes, extract_waveform_path, is_opus_stream, OPUS_CHANNELS,
    OPUS_FRAME_SIZE, OPUS_SAMPLE_RATE, OPUS_STREAM_MAGIC, WAVEFORM_MAX_BINS, WAVEFORM_MIN_BINS,
};

fn sine_pcm(secs: f64, freq: f64) -> Vec<i16> {
    let n = (secs * OPUS_SAMPLE_RATE as f64) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / OPUS_SAMPLE_RATE as f64;
            (12000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16
        })
        .collect()
}

/// Hand-rolled 16-bit mono PCM WAV (RIFF) — no external fixtures.
fn wav_bytes(pcm: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = pcm.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Stream format constants + magic sniffing
// ---------------------------------------------------------------------------

#[test]
fn constants_match_voice_note_format() {
    assert_eq!(OPUS_SAMPLE_RATE, 48_000);
    assert_eq!(OPUS_CHANNELS, 1);
    assert_eq!(OPUS_FRAME_SIZE, 960); // 20 ms @ 48 kHz
    assert_eq!(OPUS_STREAM_MAGIC, *b"SO1\0\0\0\0\0");
}

#[test]
fn is_opus_stream_accepts_magic_prefix() {
    let mut data = OPUS_STREAM_MAGIC.to_vec();
    data.push(0x00);
    assert!(is_opus_stream(&data));
}

#[test]
fn is_opus_stream_rejects_short_and_wrong_input() {
    assert!(!is_opus_stream(&[]));
    assert!(!is_opus_stream(&OPUS_STREAM_MAGIC)); // exactly 8 bytes: needs > magic len
    assert!(!is_opus_stream(b"not a voice note"));
    let mut wrong = b"XX1\0\0\0\0\0".to_vec();
    wrong.push(0x00);
    assert!(!is_opus_stream(&wrong));
}

// ---------------------------------------------------------------------------
// Voice-note codec (encode_voice_pcm / decode_voice_stream)
// ---------------------------------------------------------------------------

#[test]
fn voice_encode_decode_roundtrip() {
    let pcm = sine_pcm(1.0, 440.0);
    let stream = encode_voice_pcm(&pcm).unwrap();
    assert!(is_opus_stream(&stream));
    assert!(
        stream.len() < pcm.len() / 4,
        "opus should compress sine well: {} vs {}",
        stream.len(),
        pcm.len()
    );
    let restored = decode_voice_stream(&stream).unwrap();
    assert_eq!(restored.len(), pcm.len() - pcm.len() % OPUS_FRAME_SIZE);
    let dur = voice_duration_secs(&stream).unwrap();
    assert!((dur - 1.0).abs() < 0.05, "duration {dur}");
}

#[test]
fn voice_encode_drops_trailing_partial_frame() {
    let mut pcm = sine_pcm(0.5, 220.0);
    pcm.truncate(pcm.len() - pcm.len() % OPUS_FRAME_SIZE);
    pcm.extend(vec![0i16; 100]); // partial frame tail must be dropped
    let stream = encode_voice_pcm(&pcm).unwrap();
    let restored = decode_voice_stream(&stream).unwrap();
    assert_eq!(restored.len(), pcm.len() - 100);
}

#[test]
fn voice_encode_empty_pcm_yields_magic_only() {
    let stream = encode_voice_pcm(&[]).unwrap();
    assert_eq!(stream, OPUS_STREAM_MAGIC);
    // magic-only is not a valid stream (needs payload after header)
    assert!(!is_opus_stream(&stream));
    assert!(decode_voice_stream(&stream).is_err());
}

#[test]
fn voice_decode_rejects_non_stream() {
    assert!(decode_voice_stream(b"").is_err());
    assert!(decode_voice_stream(b"garbage").is_err());
    assert!(decode_voice_stream(&OPUS_STREAM_MAGIC).is_err());
}

#[test]
fn voice_decode_rejects_bad_framing() {
    // truncated length prefix (1 byte left)
    let mut s1 = OPUS_STREAM_MAGIC.to_vec();
    s1.push(0x01);
    assert!(decode_voice_stream(&s1).is_err());

    // zero-length packet
    let mut s2 = OPUS_STREAM_MAGIC.to_vec();
    s2.extend_from_slice(&[0x00, 0x00]);
    assert!(decode_voice_stream(&s2).is_err());

    // length prefix beyond MAX_PACKET_LEN (4096)
    let mut s3 = OPUS_STREAM_MAGIC.to_vec();
    s3.extend_from_slice(&[0xFF, 0xFF]);
    assert!(decode_voice_stream(&s3).is_err());

    // length prefix claims more bytes than remain
    let mut s4 = OPUS_STREAM_MAGIC.to_vec();
    s4.extend_from_slice(&[0x05, 0x00]);
    s4.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    assert!(decode_voice_stream(&s4).is_err());

    // well-framed but undecodable packet
    let mut s5 = OPUS_STREAM_MAGIC.to_vec();
    s5.extend_from_slice(&[0x01, 0x00]);
    s5.push(0x42);
    assert!(decode_voice_stream(&s5).is_err());
}

// ---------------------------------------------------------------------------
// voice_duration_secs / pcm_len_for_secs
// ---------------------------------------------------------------------------

#[test]
fn voice_duration_secs_matches_sample_count() {
    let stream = encode_voice_pcm(&sine_pcm(2.5, 330.0)).unwrap();
    let dur = voice_duration_secs(&stream).unwrap();
    assert!((dur - 2.5).abs() < 0.05, "duration {dur}");
}

#[test]
fn voice_duration_secs_rejects_non_stream() {
    assert!(voice_duration_secs(b"nope").is_err());
}

#[test]
fn pcm_len_for_secs_scales_with_rate() {
    assert_eq!(pcm_len_for_secs(0.0), 0);
    assert_eq!(pcm_len_for_secs(1.0), 48_000);
    assert_eq!(pcm_len_for_secs(0.5), 24_000);
    assert_eq!(pcm_len_for_secs(1.5), 72_000);
    // negative input saturates to 0 (usize cast)
    assert_eq!(pcm_len_for_secs(-1.0), 0);
}

// ---------------------------------------------------------------------------
// Waveform extraction (extract_waveform_bytes / extract_waveform_path /
// extract_waveform)
// ---------------------------------------------------------------------------

#[test]
fn waveform_from_voice_note_has_energy() {
    let stream = encode_voice_pcm(&sine_pcm(2.0, 220.0)).unwrap();
    let peaks = extract_waveform_bytes(&stream, 64).unwrap();
    assert_eq!(peaks.len(), 64);
    for p in &peaks {
        assert!((0.0..=1.0).contains(p), "peak out of range: {p}");
    }
    assert!(
        peaks.iter().any(|p| *p > 0.05),
        "sine should register energy"
    );
    assert!(peaks.iter().any(|p| *p > 0.9), "sine peak near 1.0");
}

#[test]
fn waveform_silence_is_zero() {
    let silence = vec![0i16; OPUS_FRAME_SIZE * 10];
    let stream = encode_voice_pcm(&silence).unwrap();
    let peaks = extract_waveform_bytes(&stream, 64).unwrap();
    assert!(peaks.iter().all(|p| *p == 0.0));
}

#[test]
fn waveform_bins_clamped_to_public_range() {
    let stream = encode_voice_pcm(&sine_pcm(0.5, 330.0)).unwrap();
    assert_eq!(WAVEFORM_MIN_BINS, 32);
    assert_eq!(WAVEFORM_MAX_BINS, 2048);
    assert_eq!(
        extract_waveform_bytes(&stream, 4).unwrap().len(),
        WAVEFORM_MIN_BINS
    );
    assert_eq!(
        extract_waveform_bytes(&stream, 9999).unwrap().len(),
        WAVEFORM_MAX_BINS
    );
    assert_eq!(
        extract_waveform_bytes(&stream, 0).unwrap().len(),
        WAVEFORM_MIN_BINS
    );
}

#[test]
fn waveform_rejects_empty_and_garbage_bytes() {
    assert!(extract_waveform_bytes(&[], 64).is_err());
    assert!(extract_waveform_bytes(b"not audio at all", 64).is_err());
    assert!(extract_waveform_bytes(&OPUS_STREAM_MAGIC, 64).is_err());
}

#[test]
fn waveform_from_wav_container() {
    let pcm = sine_pcm(1.0, 440.0);
    let wav = wav_bytes(&pcm, OPUS_SAMPLE_RATE);
    let dir = soshal_test_util::tmp_root("audio-core-wav");
    let path = dir.join("tone.wav");
    std::fs::write(&path, &wav).unwrap();

    let via_path = extract_waveform_path(path.to_str().unwrap(), 64).unwrap();
    assert_eq!(via_path.len(), 64);
    assert!(via_path.iter().any(|p| *p > 0.05), "wav should have energy");

    // convenience fn is path-based
    let via_conv = extract_waveform(path.to_str().unwrap(), 64).unwrap();
    assert_eq!(via_conv, via_path);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn waveform_path_missing_file_errors() {
    let err = extract_waveform_path("/nonexistent/soshal/audio.wav", 64).unwrap_err();
    assert!(err.contains("read"), "error should mention read: {err}");
}
