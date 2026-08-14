// Manual ffi tests for storage
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/storage.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-storage-manual');
  final api = env.$1;

  test('storageGetAudioPeaks returns Float32List', () {
    api.stub('crateFfiStorageStorageGetAudioPeaks', (_) => Float32List.fromList([0.1, 0.2]));
    final peaks = storageGetAudioPeaks(path: '/tmp/x');
    expect(peaks, isA<Float32List>());
    expect(api.callCount('crateFfiStorageStorageGetAudioPeaks'), 1);
  });

  test('storageEncodeVoicePcm and decode/voice duration', () {
    api.stub('crateFfiStorageStorageEncodeVoicePcm', (_) => Uint8List.fromList([1,2]));
    api.stub('crateFfiStorageStorageDecodeVoiceStream', (_) => Int16List.fromList([1,2]));
    api.stub('crateFfiStorageStorageVoiceDurationSecs', (_) => 1.23);
    final enc = storageEncodeVoicePcm(pcm: [1,2,3]);
    final dec = storageDecodeVoiceStream(payload: [1,2]);
    final dur = storageVoiceDurationSecs(payload: [1,2]);
    expect(enc, isA<Uint8List>());
    expect(dec, isA<Int16List>());
    expect(dur, isA<double>());
    expect(api.callCount('crateFfiStorageStorageEncodeVoicePcm'), 1);
    expect(api.callCount('crateFfiStorageStorageDecodeVoiceStream'), 1);
  });

  test('storageGetIoEngineMode', () {
    api.stubString('crateFfiStorageStorageGetIoEngineMode', 'StandardTokioFs');
    final mode = storageGetIoEngineMode();
    expect(mode, 'StandardTokioFs');
    expect(api.callCount('crateFfiStorageStorageGetIoEngineMode'), 1);
  });
}
