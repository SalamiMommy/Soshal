use soshal_audio_core::voice::{decode_voice_stream, encode_voice_pcm, voice_duration_secs};
use soshal_audio_core::{
    extract_waveform, extract_waveform_bytes, extract_waveform_path, is_opus_stream,
    OPUS_FRAME_SIZE, OPUS_SAMPLE_RATE,
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

fn wav_bytes(pcm: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + (pcm.len() as u32) * 2).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&OPUS_SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(OPUS_SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() * 2).to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[test]
fn voice_encode_decode_roundtrip() {
    let pcm = sine_pcm(1.0, 440.0);
    let stream = encode_voice_pcm(&pcm).unwrap();
    assert!(is_opus_stream(&stream));
    assert!(stream.len() < pcm.len(), "opus should beat raw pcm");
    let restored = decode_voice_stream(&stream).unwrap();
    assert!(restored.len() >= pcm.len() - OPUS_FRAME_SIZE * 2);
    let dur = voice_duration_secs(&stream).unwrap();
    assert!((dur - 1.0).abs() < 0.05, "duration {dur}");
}

#[test]
fn voice_stream_rejects_garbage() {
    assert!(decode_voice_stream(b"not a voice note").is_err());
    assert!(voice_duration_secs(b"not a voice note").is_err());
}

#[test]
fn waveform_from_voice_stream() {
    let pcm = sine_pcm(2.0, 220.0);
    let stream = encode_voice_pcm(&pcm).unwrap();
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
fn waveform_bins_clamped() {
    let pcm = sine_pcm(0.5, 330.0);
    let stream = encode_voice_pcm(&pcm).unwrap();
    assert_eq!(extract_waveform_bytes(&stream, 4).unwrap().len(), 32);
    assert_eq!(extract_waveform_bytes(&stream, 9999).unwrap().len(), 2048);
}

#[test]
fn waveform_silence_is_zero() {
    let silence = vec![0i16; OPUS_FRAME_SIZE * 10];
    let stream = encode_voice_pcm(&silence).unwrap();
    let peaks = extract_waveform_bytes(&stream, 64).unwrap();
    assert!(peaks.iter().all(|p| *p == 0.0));
}

#[test]
fn waveform_path_roundtrip() {
    let pcm = sine_pcm(0.5, 440.0);
    let stream = encode_voice_pcm(&pcm).unwrap();
    let dir = soshal_test_util::tmp_root("audio");
    let path = dir.join("voice.wav");
    std::fs::write(&path, &stream).unwrap();
    let peaks = extract_waveform_path(path.to_str().unwrap(), 64).unwrap();
    assert_eq!(peaks.len(), 64);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn waveform_from_wav_container() {
    let pcm = sine_pcm(0.5, 330.0);
    let wav = wav_bytes(&pcm);
    let peaks = extract_waveform_bytes(&wav, 32).unwrap();
    assert_eq!(peaks.len(), 32);
    assert!(peaks.iter().any(|p| *p > 0.05), "sine energy via symphonia");
}

#[test]
fn waveform_wav_path_with_extension_hint() {
    let pcm = sine_pcm(0.25, 220.0);
    let dir = soshal_test_util::tmp_root("audio-wav");
    let path = dir.join("note.wav");
    std::fs::write(&path, wav_bytes(&pcm)).unwrap();
    let peaks = extract_waveform(path.to_str().unwrap(), 32).unwrap();
    assert_eq!(peaks.len(), 32);
    assert!(peaks.iter().any(|p| *p > 0.05));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Real FLAC container (Symphonia `flac` feature path): a 1 s 440 Hz sine
/// fixture generated with ffmpeg, same include_bytes pattern as the
/// moderation-core video fixtures. Pins the Musicloud upload + waveform
/// path for lossless FLAC files.
#[test]
fn waveform_from_flac_container() {
    const FLAC: &[u8] = include_bytes!("fixtures/sine_440_1s.flac");
    assert!(
        FLAC.len() > 4 && &FLAC[..4] == b"fLaC",
        "fixture is a flac stream"
    );
    let peaks = extract_waveform_bytes(FLAC, 64).unwrap();
    assert_eq!(peaks.len(), 64);
    for p in &peaks {
        assert!((0.0..=1.0).contains(p), "peak out of range: {p}");
    }
    assert!(
        peaks.iter().any(|p| *p > 0.05),
        "sine should register energy"
    );
    assert!(peaks.iter().any(|p| *p > 0.9), "sine peak near 1.0");
    // Same file via the path API (extension hint routes the prober to FLAC).
    let dir = soshal_test_util::tmp_root("audio-flac");
    let path = dir.join("track.flac");
    std::fs::write(&path, FLAC).unwrap();
    let path_peaks = extract_waveform_path(path.to_str().unwrap(), 64).unwrap();
    assert_eq!(path_peaks.len(), 64);
    assert!(path_peaks.iter().any(|p| *p > 0.05));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn waveform_rejects_garbage_bytes() {
    assert!(extract_waveform_bytes(b"not audio at all", 32).is_err());
    assert!(extract_waveform_path("/nonexistent/soshal.wav", 32).is_err());
}

#[test]
fn waveform_silent_wav_is_zero() {
    let silence = vec![0i16; OPUS_SAMPLE_RATE as usize / 2];
    let wav = wav_bytes(&silence);
    let peaks = extract_waveform_bytes(&wav, 32).unwrap();
    assert_eq!(peaks.len(), 32);
    assert!(peaks.iter().all(|p| *p == 0.0), "silence floors to 0");
}
