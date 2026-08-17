//! Storage FFI module
//! Audio waveforms, voice-note codec, eviction

use flutter_rust_bridge::frb;
use soshal_audio_core::extract_waveform_path;
use soshal_audio_core::voice::{decode_voice_stream, encode_voice_pcm, voice_duration_secs};

/// Waveform peaks (normalized 0..1) for an audio file or voice-note stream.
#[frb(serialize)]
pub async fn storage_get_audio_peaks(path: String) -> Result<Vec<f32>, String> {
    extract_waveform_path(&path, 64).into()
}

/// Encode mono i16 PCM (48 kHz) into a framed Opus voice-note stream.
#[frb(serialize)]
pub async fn storage_encode_voice_pcm(pcm: Vec<i16>) -> Result<Vec<u8>, String> {
    encode_voice_pcm(&pcm).into()
}

/// Decode a framed Opus voice-note stream to mono i16 PCM.
#[frb(sync, serialize)]
pub fn storage_decode_voice_stream(payload: Vec<u8>) -> Result<Vec<i16>, String> {
    decode_voice_stream(&payload).into()
}

/// Duration in seconds of a voice-note stream.
#[frb(sync, serialize)]
pub fn storage_voice_duration_secs(payload: Vec<u8>) -> Result<f64, String> {
    voice_duration_secs(&payload).into()
}

/// Detect and return active storage I/O engine mode (IoUringKernelRing, MemmapZeroCopy, StandardTokioFs).
#[frb(sync, serialize)]
pub fn storage_get_io_engine_mode() -> Result<String, String> {
    let engine = soshal_storage_core::io_uring_backend::IoUringEngine::new();
    Ok(format!("{:?}", engine.mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_pcm(secs: f64, freq: f64) -> Vec<i16> {
        let n = (secs * 48000.0) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / 48000.0;
                (12000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16
            })
            .collect()
    }

    #[tokio::test]
    async fn test_voice_pcm_roundtrip() {
        let pcm = sine_pcm(1.0, 440.0);
        let stream = storage_encode_voice_pcm(pcm.clone()).await.unwrap();
        assert!(soshal_audio_core::is_opus_stream(&stream));
        assert!(stream.len() < pcm.len(), "opus should beat raw pcm");
        let restored = storage_decode_voice_stream(stream.clone()).unwrap();
        assert!(restored.len() >= pcm.len() - soshal_audio_core::OPUS_FRAME_SIZE * 2);
        let dur = storage_voice_duration_secs(stream).unwrap();
        assert!((dur - 1.0).abs() < 0.05, "duration {dur}");
    }

    #[tokio::test]
    async fn test_voice_stream_rejects_garbage() {
        assert!(storage_decode_voice_stream(b"not a voice note".to_vec()).is_err());
        assert!(storage_voice_duration_secs(b"not a voice note".to_vec()).is_err());
    }

    #[tokio::test]
    async fn test_voice_stream_rejects_corrupt_framing() {
        let mut bad = soshal_audio_core::OPUS_STREAM_MAGIC.to_vec();
        bad.push(0x01);
        assert!(storage_decode_voice_stream(bad).is_err());
        let mut oversized = soshal_audio_core::OPUS_STREAM_MAGIC.to_vec();
        oversized.extend_from_slice(&0xFFFFu16.to_le_bytes());
        assert!(storage_decode_voice_stream(oversized).is_err());
    }

    #[tokio::test]
    async fn test_voice_pcm_empty_encodes_magic_only() {
        let stream = storage_encode_voice_pcm(vec![]).await.unwrap();
        assert_eq!(stream, soshal_audio_core::OPUS_STREAM_MAGIC.to_vec());
    }

    #[test]
    fn test_io_engine_mode_ok() {
        let mode = storage_get_io_engine_mode().unwrap();
        assert!(
            mode == "IoUringKernelRing" || mode == "MemmapZeroCopy" || mode == "StandardTokioFs",
            "unexpected mode {mode}"
        );
    }

    #[tokio::test]
    async fn test_audio_peaks_missing_file_errors() {
        let path = soshal_test_util::tmp_path("storage", "no-such-file.wav");
        assert!(storage_get_audio_peaks(path.to_string_lossy().to_string())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_audio_peaks_wav_fixture() {
        let pcm = sine_pcm(2.0, 220.0);
        let data_len = (pcm.len() * 2) as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&96000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for s in &pcm {
            wav.extend_from_slice(&s.to_le_bytes());
        }
        let path = soshal_test_util::tmp_path("storage", "fixture.wav");
        std::fs::write(&path, &wav).unwrap();
        let peaks = storage_get_audio_peaks(path.to_string_lossy().to_string())
            .await
            .unwrap();
        let _ = std::fs::remove_file(&path);
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
}
