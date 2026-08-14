// ignore_for_file: invalid_use_of_internal_member
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/audio_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-audio');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('AudioService', () {
    test('peaksFor parses RMS envelope and passes path', () {
      final audio = AudioService();
      api.stub(
        'crateFfiStorageStorageGetAudioPeaks',
        (_) => <double>[0.1, 0.5, 0.9],
      );

      final peaks = audio.peaksFor('/voice/a.m4a');
      expect(peaks.length, 3, reason: 'bucket count');
      expect(peaks[0], 0.1);
      expect(peaks[2], 0.9);
      final inv =
          api.callsOf('crateFfiStorageStorageGetAudioPeaks').single;
      expect(api.namedArg(inv, 'path'), '/voice/a.m4a');
    });

    test('peaksFor swallow errors and return empty', () {
      final audio = AudioService();
      api.stub('crateFfiStorageStorageGetAudioPeaks',
          (_) => throw Exception('read failed'));

      expect(audio.peaksFor('/voice/bad.m4a'), isEmpty);
    });

    test('encodeVoice passes pcm payload through FFI', () {
      final audio = AudioService();
      api.stub(
        'crateFfiStorageStorageEncodeVoicePcm',
        (_) => Uint8List.fromList([0x01, 0x02, 0x03]),
      );
      final pcm = [100, 200, -100, 0];

      final encoded = audio.encodeVoice(pcm);
      expect(encoded, [0x01, 0x02, 0x03]);
      final inv =
          api.callsOf('crateFfiStorageStorageEncodeVoicePcm').single;
      expect(api.namedArg(inv, 'pcm'), pcm);
    });

    test('encodeVoice rethrows FFI errors', () {
      final audio = AudioService();
      api.stub('crateFfiStorageStorageEncodeVoicePcm',
          (_) => throw Exception('encode failed'));

      expect(() => audio.encodeVoice([1, 2]), throwsException);
    });

    test('decodeVoice passes payload and returns Int16List', () {
      final audio = AudioService();
      api.stub(
        'crateFfiStorageStorageDecodeVoiceStream',
        (_) => Int16List.fromList([7, 8, 9]),
      );
      final payload = Uint8List.fromList([0xaa, 0xbb]);

      final decoded = audio.decodeVoice(payload);
      expect(decoded, [7, 8, 9]);
      final inv =
          api.callsOf('crateFfiStorageStorageDecodeVoiceStream').single;
      expect(api.namedArg(inv, 'payload'), payload);
    });

    test('decodeVoice rethrows FFI errors', () {
      final audio = AudioService();
      api.stub('crateFfiStorageStorageDecodeVoiceStream',
          (_) => throw Exception('decode failed'));

      expect(
        () => audio.decodeVoice(Uint8List(0)),
        throwsException,
      );
    });

    test('durationSecs returns playback seconds', () {
      final audio = AudioService();
      api.stub(
        'crateFfiStorageStorageVoiceDurationSecs',
        (_) => 2.5,
      );
      final payload = Uint8List.fromList([0x01]);

      expect(audio.durationSecs(payload), 2.5);
      final inv =
          api.callsOf('crateFfiStorageStorageVoiceDurationSecs').single;
      expect(api.namedArg(inv, 'payload'), payload);
    });

    test('durationSecs returns 0 on FFI error', () {
      final audio = AudioService();
      api.stub('crateFfiStorageStorageVoiceDurationSecs',
          (_) => throw Exception('duration failed'));

      expect(audio.durationSecs(Uint8List(0)), 0);
    });
  });
}