//! FFI surface for live audio (AAudio + AMediaCodec AAC via codecs/audio.rs).
//! All fns are `#[frb(sync, serialize)]`; off-Android they return defaults.

use crate::codecs;
use flutter_rust_bridge::frb;

/// True only on Android ≥ 26 (AAudio + AImage gate).
#[frb(sync, serialize)]
pub fn audio_is_supported() -> bool {
    codecs::audio::is_supported()
}

/// Start mic + AAC-LC encoder + capture thread. Caller must
/// `audio_set_mic_enable(true)` before audio flows.
#[frb(sync, serialize)]
pub fn audio_init_encode() -> bool {
    codecs::audio::init_encode()
}

/// Toggle the mic. Returns immediately; keep polling `audio_drain`.
#[frb(sync, serialize)]
pub fn audio_set_mic_enable(on: bool) -> bool {
    codecs::audio::set_mic_enable(on)
}

/// Drain queued AAC blobs: `[2, ...config]` or `[1, ...frame]`.
#[frb(sync, serialize)]
pub fn audio_drain() -> Vec<Vec<u8>> {
    codecs::audio::drain()
}

/// Set up the AAC decoder + AAudio output. Safe to call once per viewer
/// session.
#[frb(sync, serialize)]
pub fn audio_init_decode() -> bool {
    codecs::audio::init_decode()
}

/// Feed one AAC blob (config or frame); decoded PCM plays immediately.
///
/// Runs on the blocking pool, not the UI isolate. `feed_aac` ends in
/// `AAudioStream_write` with a 10-second timeout, so a stalled audio sink
/// would otherwise freeze the Flutter UI thread for as long as that timeout.
/// Callers already `await` each frame in sequence, and the codec state lives
/// behind a mutex, so moving the thread preserves feed order.
#[frb(serialize)]
pub async fn audio_feed_aac(blob: Vec<u8>) -> bool {
    tokio::task::spawn_blocking(move || codecs::audio::feed_aac(&blob))
        .await
        .unwrap_or(false)
}

/// Stop mic, codecs, playback; release everything.
#[frb(sync, serialize)]
pub fn audio_release() -> bool {
    codecs::audio::release()
}
