import 'dart:io' show Platform;

import 'package:flutter/services.dart';

/// Thin Dart wrapper over the native H.264 codec channel (`com.soshal/h264`,
/// implemented in `MainActivity.kt` via `H264Codec.kt` — MediaCodec).
///
/// Encode: feed BGRA frame bytes, get drained Annex-B NAL blobs, each tagged
/// with a leading key-frame flag (`1` = key frame, `0` = delta).
/// Decode: feed Annex-B NAL blobs (one MoQ object payload each), get JPEG
/// byte arrays back (one per drained video frame).
///
/// Every call is a no-op / returns empty on non-Android or when the channel
/// is missing — callers fall back to the JPEG path.
class H264Codec {
  H264Codec._();

  static const MethodChannel _channel = MethodChannel('com.soshal/h264');

  static bool? _supported;

  /// True only when running on Android AND the native encoder is present.
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

  /// Configure the hardware AVC encoder (I420 in via BGRA conversion).
  static Future<bool> initEncode({
    required int width,
    required int height,
    int bitrate = 800000,
    int fps = 15,
  }) async {
    try {
      return await _channel.invokeMethod<bool>('initEncode', {
            'width': width,
            'height': height,
            'bitrate': bitrate,
            'fps': fps,
          }) ??
          false;
    } catch (_) {
      return false;
    }
  }

  /// Feed one BGRA frame (width*height*4 bytes). Returns drained NAL blobs,
  /// each `[flag, ...annexB]` (Uint8List of 1 + N bytes).
  static Future<List<Uint8List>> feedEncode(Uint8List bgra) async {
    try {
      final out =
          await _channel.invokeListMethod<Uint8List>('feedEncode', bgra);
      return out ?? const [];
    } catch (_) {
      return const [];
    }
  }

  /// Configure the decoder (software AVC; feed Annex-B directly).
  static Future<bool> initDecode() async {
    try {
      return await _channel.invokeMethod<bool>('initDecode') ?? false;
    } catch (_) {
      return false;
    }
  }

  /// Feed one Annex-B NAL blob; returns JPEG frames drained from the decoder.
  static Future<List<Uint8List>> feedDecode(Uint8List nal) async {
    try {
      final out = await _channel.invokeListMethod<Uint8List>('feedDecode', nal);
      return out ?? const [];
    } catch (_) {
      return const [];
    }
  }

  /// Start the local DVR: muxes the live H.264/AAC bleed into an MP4 under
  /// `<filesDir>/recordings/`. Returns the output path or null on failure.
  /// Safe to call while broadcasting (recording is off until this returns).
  static Future<String?> initRecord() async {
    try {
      return await _channel.invokeMethod<String>('initRecord');
    } catch (_) {
      return null;
    }
  }

  /// Stop the DVR and seal the MP4. Returns the recorded file path (or null
  /// when nothing was recorded).
  static Future<String?> stopRecord() async {
    try {
      return await _channel.invokeMethod<String>('stopRecord');
    } catch (_) {
      return null;
    }
  }

  /// Stop + release both encoder and decoder if initialized.
  static Future<void> release() async {
    try {
      await _channel.invokeMethod<void>('release');
    } catch (_) {
      // channel missing — nothing to release
    }
  }
}
