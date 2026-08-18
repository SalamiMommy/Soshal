//! FFI surface for the H.264 codec (NDK AMediaCodec via codecs/h264.rs).
//! All fns are `#[frb(sync, serialize)]`; off-Android they return the
//! default (false / empty / None) so the Dart wrappers can stay thin.

use crate::codecs;
use flutter_rust_bridge::frb;

/// True only on Android ≥ 26 with a hardware AVC encoder present.
#[frb(sync, serialize)]
pub fn h264_is_supported() -> bool {
    codecs::h264::is_supported()
}

/// Configure the hardware AVC encoder (I420 in via BGRA conversion).
#[frb(sync, serialize)]
pub fn h264_init_encode(width: i32, height: i32, bitrate: i32, fps: i32) -> bool {
    codecs::h264::init_encode(width, height, bitrate, fps)
}

/// Feed one BGRA frame; returns drained `[flag, ...Annex-B]` blobs.
#[frb(sync, serialize)]
pub fn h264_feed_encode(bgra: Vec<u8>) -> Vec<Vec<u8>> {
    codecs::h264::feed_encode(&bgra)
}

/// Configure the software AVC decoder (feed Annex-B directly).
#[frb(sync, serialize)]
pub fn h264_init_decode() -> bool {
    codecs::h264::init_decode()
}

/// Feed one Annex-B NAL blob; returns JPEG frames drained from the decoder.
#[frb(sync, serialize)]
pub fn h264_feed_decode(nal: Vec<u8>) -> Vec<Vec<u8>> {
    codecs::h264::feed_decode(&nal)
}

/// Start the local DVR (MP4 muxer, Kotlin LiveRecorder). Returns the output
/// path or None on failure.
#[frb(sync, serialize)]
pub fn h264_init_record() -> Option<String> {
    codecs::dvr_start()
}

/// Stop the DVR and seal the MP4. Returns the recorded file path (or None
/// when nothing was recorded).
#[frb(sync, serialize)]
pub fn h264_stop_record() -> Option<String> {
    codecs::dvr_stop()
}

/// Stop + release both encoder and decoder if initialized.
#[frb(sync, serialize)]
pub fn h264_release() -> bool {
    codecs::h264::release()
}
