import 'dart:typed_data';

import '../ffi/audio.dart' as ffi_audio;

/// Dart wrapper over the Rust AAC codec (codecs/audio.rs — AAudio mic +
/// AMediaCodec AAC-LC encode, AMediaCodec decode → AAudio playback).
///
/// Encode side runs on a Rust background thread; `drainAudio` returns the
/// queued blobs as `[tag, ...aac]` (tag 2 = codec config, tag 1 = frame).
/// Decode side is fed via `feedAac` and plays through the device speaker.
///
/// Every call is a no-op off-Android — callers fall back to video-only
/// streams.
class AudioCodec {
  AudioCodec._();

  static bool? _supported;

  /// True only on Android ≥ 26 with the native bridge available.
  static Future<bool> isSupported() async {
    final cached = _supported;
    if (cached != null) return cached;
    try {
      final ok = ffi_audio.audioIsSupported();
      _supported = ok;
      return ok;
    } catch (_) {
      _supported = false;
      return false;
    }
  }

  /// Start the mic + AAC encoder (48 kHz mono, 64 kbps). Caller must
  /// `setMicEnable(true)` before audio flows.
  static Future<bool> initEncode() async {
    try {
      return ffi_audio.audioInitEncode();
    } catch (_) {
      return false;
    }
  }

  /// Toggle the mic. Returns immediately; keep polling `drainAudio`.
  static Future<void> setMicEnable(bool on) async {
    try {
      ffi_audio.audioSetMicEnable(on_: on);
    } catch (_) {
      // bridge missing
    }
  }

  /// Drain queued AAC blobs: `[2, ...config]` or `[1, ...frame]`.
  static Future<List<Uint8List>> drainAudio() async {
    try {
      return ffi_audio.audioDrain();
    } catch (_) {
      return const [];
    }
  }

  /// Set up the decoder + audio output. Safe to call once per viewer session.
  static Future<bool> initDecode() async {
    try {
      return ffi_audio.audioInitDecode();
    } catch (_) {
      return false;
    }
  }

  /// Feed one AAC blob (config or frame); decoded PCM plays immediately.
  static Future<void> feedAac(List<int> aac) async {
    try {
      final bytes = aac is Uint8List ? aac : Uint8List.fromList(aac);
      ffi_audio.audioFeedAac(blob: bytes);
    } catch (_) {
      // bridge missing
    }
  }

  /// Stop mic, codecs, playback; release everything.
  static Future<void> release() async {
    try {
      ffi_audio.audioRelease();
    } catch (_) {
      // bridge missing
    }
  }
}
