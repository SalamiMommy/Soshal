//! Voice-note Opus codec. Storage format: `[magic 8B][u16 LE len][packet]...`
//! — length-prefixed raw Opus packets so the decoder can split without a
//! container. 48 kHz mono, 20 ms frames (960 samples).

use audiopus::coder::{Decoder, Encoder};
use audiopus::{Channels, SampleRate};

use crate::{OPUS_FRAME_SIZE, OPUS_SAMPLE_RATE};

/// Max single Opus packet size; bounds the length prefix.
const MAX_PACKET_LEN: usize = 4096;

fn new_decoder() -> Result<Decoder, String> {
    Decoder::new(SampleRate::Hz48000, Channels::Mono).map_err(|e| format!("opus decoder: {e}"))
}

fn new_encoder() -> Result<Encoder, String> {
    Encoder::new(
        SampleRate::Hz48000,
        Channels::Mono,
        audiopus::Application::Voip,
    )
    .map_err(|e| format!("opus encoder: {e}"))
}

/// Decode a length-prefixed Opus stream (after the magic header) to mono f32.
pub(crate) fn decode_packets(data: &[u8]) -> Result<crate::MonoF32, String> {
    let mut dec = new_decoder()?;
    let est_frames = (data.len() / 32).max(1);
    let mut out = Vec::with_capacity(est_frames * OPUS_FRAME_SIZE);
    let mut pos = 0usize;
    let mut pcm = vec![0i16; OPUS_FRAME_SIZE];
    while pos < data.len() {
        if data.len() - pos < 2 {
            return Err("truncated opus length prefix".to_string());
        }
        let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if len == 0 || len > MAX_PACKET_LEN || data.len() - pos < len {
            return Err("invalid opus packet length".to_string());
        }
        let packet = &data[pos..pos + len];
        pos += len;
        let n = dec
            .decode(Some(packet), &mut pcm[..], false)
            .map_err(|e| format!("opus decode: {e}"))?;
        out.extend(pcm[..n].iter().map(|s| *s as f32 / 32768.0));
    }
    Ok(out)
}

/// Encode i16 PCM (48 kHz mono, 960-sample frames) into a framed voice-note
/// stream, magic header included.
pub fn encode_voice_pcm(pcm: &[i16]) -> Result<Vec<u8>, String> {
    let enc = new_encoder()?;
    let est_packets = pcm.len() / OPUS_FRAME_SIZE;
    let mut out = Vec::with_capacity(8 + est_packets * 64);
    out.extend_from_slice(&crate::OPUS_STREAM_MAGIC);
    let mut packet_buf = vec![0u8; MAX_PACKET_LEN];
    for chunk in pcm.chunks(OPUS_FRAME_SIZE) {
        if chunk.len() < OPUS_FRAME_SIZE {
            break; // drop trailing partial frame
        }
        let n = enc
            .encode(chunk, &mut packet_buf[..])
            .map_err(|e| format!("opus encode: {e}"))?;
        if n > MAX_PACKET_LEN {
            return Err("opus packet exceeds max length".to_string());
        }
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&packet_buf[..n]);
    }
    Ok(out)
}

/// Decode a framed voice-note stream to i16 PCM (48 kHz mono).
pub fn decode_voice_stream(data: &[u8]) -> Result<Vec<i16>, String> {
    if !crate::is_opus_stream(data) {
        return Err("not a voice-note stream".to_string());
    }
    let mono = decode_packets(&data[crate::OPUS_STREAM_MAGIC.len()..])?;
    Ok(mono
        .into_iter()
        .map(|f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect())
}

/// Duration in seconds of a voice-note stream. Derived from the packet count
/// (fixed 20 ms frames) instead of a full decode.
pub fn voice_duration_secs(data: &[u8]) -> Result<f64, String> {
    if !crate::is_opus_stream(data) {
        return Err("not a voice-note stream".to_string());
    }
    let mut packets = 0u64;
    let mut pos = crate::OPUS_STREAM_MAGIC.len();
    while pos < data.len() {
        if data.len() - pos < 2 {
            return Err("truncated opus length prefix".to_string());
        }
        let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if len == 0 || len > MAX_PACKET_LEN || data.len() - pos < len {
            return Err("invalid opus packet length".to_string());
        }
        pos += len;
        packets += 1;
    }
    Ok(packets as f64 * (OPUS_FRAME_SIZE as f64 / OPUS_SAMPLE_RATE as f64))
}

/// Constants for recorders: samples needed for `secs` of audio.
pub fn pcm_len_for_secs(secs: f64) -> usize {
    (secs * OPUS_SAMPLE_RATE as f64).round() as usize
}
