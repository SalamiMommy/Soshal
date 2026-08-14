import 'dart:io' show Platform;

import 'package:flutter/services.dart';

/// Dart wrapper over the native AAC codec channel (`com.soshal/audio`,
/// implemented in `MainActivity.kt` via `AudioCodec.kt` — AudioRecord +
/// MediaCodec AAC-LC encode, MediaCodec decode -> AudioTrack).
///
/// Encode side runs on a native background thread; `drainAudio` returns the
/// queued blobs as `[tag, ...aac]` (tag 2 = codec config, tag 1 = frame).
/// Decode side is fed via `feedAac` and plays through the device speaker.
///
/// Every call is a no-op on non-Android or when the channel is missing —
/// callers fall back to video-only streams.
class AudioCodec {
  AudioCodec._();

  static const MethodChannel _channel = MethodChannel('com.soshal/audio');

  static bool? _supported;

  /// True only on Android with a live native channel.
  static Future<bool> isSupported() async {
    if (!Platform.isAndroid) return false;
    final cached = _supported;
    if (cached != null) return cached;
    try {
      final ok = await _channel.invokeMethod<bool>('isSupported') ?? false;
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
      return await _channel.invokeMethod<bool>('initEncode') ?? false;
    } catch (_) {
      return false;
    }
  }

  /// Toggle the mic. Returns immediately; keep polling `drainAudio`.
  static Future<void> setMicEnable(bool on) async {
    try {
      await _channel.invokeMethod<void>('setMicEnable', on);
    } catch (_) {
      // channel missing
    }
  }

  /// Drain queued AAC blobs: `[2, ...config]` or `[1, ...frame]`.
  static Future<List<Uint8List>> drainAudio() async {
    try {
      final out = await _channel.invokeListMethod<Uint8List>('drainAudio');
      return out ?? const [];
    } catch (_) {
      return const [];
    }
  }

  /// Set up the decoder + AudioTrack. Safe to call once per viewer session.
  static Future<bool> initDecode() async {
    try {
      return await _channel.invokeMethod<bool>('initDecode') ?? false;
    } catch (_) {
      return false;
    }
  }

  /// Feed one AAC blob (config or frame); decoded PCM plays immediately.
  static Future<void> feedAac(List<int> aac) async {
    try {
      final bytes = aac is Uint8List ? aac : Uint8List.fromList(aac);
      await _channel.invokeMethod<void>('feedAac', bytes);
    } catch (_) {
      // channel missing
    }
  }

  /// Stop mic, codecs, playback; release everything.
  static Future<void> release() async {
    try {
      await _channel.invokeMethod<void>('release');
    } catch (_) {
      // channel missing
    }
  }
}
