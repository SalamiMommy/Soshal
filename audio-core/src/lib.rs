//! Audio pipeline core: waveform extraction, Opus encode/decode.
//!
//! Waveform extraction is container-agnostic (symphonia: mp3/m4a/flac/wav/
//! ogg-vorbis/ogg-opus) with a raw-Opus fallback (audiopus) for voice notes
//! stored as bare packet streams. Output is a normalized peak array — Flutter
//! draws it, never crunches audio.

pub mod voice;
pub mod waveform;

pub use voice::decode_voice_stream;
pub use waveform::{
    extract_waveform, extract_waveform_bytes, extract_waveform_path, WAVEFORM_MAX_BINS,
    WAVEFORM_MIN_BINS,
};

pub const OPUS_SAMPLE_RATE: u32 = 48_000;
pub const OPUS_CHANNELS: usize = 1;
pub const OPUS_FRAME_SIZE: usize = 960;

/// Raw Opus packet-stream envelope: `[magic 8B][u16 LE len][packet]...`.
/// Marks the stream as our voice-note format so waveform extraction can
/// route to the audiopus decoder instead of a container parser.
pub const OPUS_STREAM_MAGIC: [u8; 8] = *b"SO1\0\0\0\0\0";

/// True when `data` carries the raw Opus voice-note envelope.
pub fn is_opus_stream(data: &[u8]) -> bool {
    data.len() > OPUS_STREAM_MAGIC.len() && data[..OPUS_STREAM_MAGIC.len()] == OPUS_STREAM_MAGIC
}

/// Decoded mono f32 samples, -1..1.
pub(crate) type MonoF32 = Vec<f32>;
