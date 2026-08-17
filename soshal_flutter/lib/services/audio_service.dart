// ignore_for_file: invalid_use_of_internal_member
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Voice memo waveform + codec helpers. All heavy DSP lives in Rust
/// (storage-core); this service is a thin FFI wrapper for the composer UI.
class AudioService {
  /// RMS peak envelope (0..1) for a voice file — waveform rendering.
  Future<List<double>> peaksFor(String path, {int buckets = 64}) async {
    try {
      return (await RustLib.instance.api
              .crateFfiStorageStorageGetAudioPeaks(path: path))
          .toList();
    } catch (e) {
      debugPrint('audio peaks: $e');
      return const [];
    }
  }

  /// Compress PCM (i16) into the wire codec (voice memos).
  Future<Uint8List> encodeVoice(List<int> pcm) async {
    try {
      return await RustLib.instance.api
          .crateFfiStorageStorageEncodeVoicePcm(pcm: pcm);
    } catch (e) {
      debugPrint('voice encode: $e');
      rethrow;
    }
  }

  /// Decompress a wire-encoded voice payload back to i16 PCM.
  Int16List decodeVoice(Uint8List payload) {
    try {
      return RustLib.instance.api
          .crateFfiStorageStorageDecodeVoiceStream(payload: payload);
    } catch (e) {
      debugPrint('voice decode: $e');
      rethrow;
    }
  }

  /// Playback duration in seconds of a wire-encoded voice payload.
  double durationSecs(Uint8List payload) {
    try {
      return RustLib.instance.api
          .crateFfiStorageStorageVoiceDurationSecs(payload: payload);
    } catch (e) {
      debugPrint('voice duration: $e');
      return 0;
    }
  }
}
