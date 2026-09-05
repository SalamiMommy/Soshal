import 'package:flutter/foundation.dart';

import '../ffi/h264.dart' as ffi_h264;

/// Thin Dart wrapper over the Rust H.264 codec (codecs/h264.rs, NDK
/// AMediaCodec). Off-Android the bridge returns defaults, so callers fall
/// back to the JPEG path.
///
/// Encode: feed BGRA frame bytes, get drained Annex-B NAL blobs, each tagged
/// with a leading key-frame flag (`1` = key frame, `0` = delta).
/// Decode: feed Annex-B NAL blobs (one MoQ object payload each), get JPEG
/// byte arrays back (one per drained video frame).
class H264Codec {
  H264Codec._();

  static bool? _supported;

  /// Set when a real FFI encode/decode call fails; cleared on the next
  /// successful call. Off-Android never sets this (callers gate on
  /// `isSupported()` first, so `const []` stays the legitimate no-op).
  static final ValueNotifier<String?> error = ValueNotifier<String?>(null);

  /// True only when running on Android ≥ 26 AND the native encoder exists.
  static Future<bool> isSupported() async {
    final cached = _supported;
    if (cached != null) return cached;
    try {
      final ok = ffi_h264.h264IsSupported();
      _supported = ok;
      return ok;
    } catch (_) {
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
      return ffi_h264.h264InitEncode(
        width: width,
        height: height,
        bitrate: bitrate,
        fps: fps,
      );
    } catch (_) {
      return false;
    }
  }

  /// Feed one BGRA frame (width*height*4 bytes). Returns drained NAL blobs,
  /// each `[flag, ...annexB]` (Uint8List of 1 + N bytes).
  static Future<List<Uint8List>> feedEncode(Uint8List bgra) async {
    try {
      final blobs = ffi_h264.h264FeedEncode(bgra: bgra);
      error.value = null;
      return blobs;
    } catch (e) {
      error.value = 'h264 encode failed: $e';
      return const [];
    }
  }

  /// Configure the decoder (software AVC; feed Annex-B directly).
  static Future<bool> initDecode() async {
    try {
      return ffi_h264.h264InitDecode();
    } catch (_) {
      return false;
    }
  }

  /// Feed one Annex-B NAL blob; returns JPEG frames drained from the decoder.
  static Future<List<Uint8List>> feedDecode(Uint8List nal) async {
    try {
      final jpegs = ffi_h264.h264FeedDecode(nal: nal);
      error.value = null;
      return jpegs;
    } catch (e) {
      error.value = 'h264 decode failed: $e';
      return const [];
    }
  }

  /// Start the local DVR: muxes the live H.264/AAC bleed into an MP4 under
  /// `<filesDir>/recordings/`. Returns the output path or null on failure.
  /// Safe to call while broadcasting (recording is off until this returns).
  static Future<String?> initRecord() async {
    try {
      return ffi_h264.h264InitRecord();
    } catch (_) {
      return null;
    }
  }

  /// Stop the DVR and seal the MP4. Returns the recorded file path (or null
  /// when nothing was recorded).
  static Future<String?> stopRecord() async {
    try {
      return ffi_h264.h264StopRecord();
    } catch (_) {
      return null;
    }
  }

  /// Stop + release both encoder and decoder if initialized.
  static Future<void> release() async {
    try {
      ffi_h264.h264Release();
    } catch (_) {
      // bridge missing — nothing to release
    }
  }
}
