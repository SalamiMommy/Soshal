// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/audio_codec.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-audio-codec');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('AudioCodec off-Android', () {
    test('isSupported returns false without touching the channel', () async {
      expect(await AudioCodec.isSupported(), isFalse);
    });

    test('initEncode returns false', () async {
      expect(await AudioCodec.initEncode(), isFalse);
    });

    test('setMicEnable completes for on and off', () async {
      await AudioCodec.setMicEnable(true);
      await AudioCodec.setMicEnable(false);
    });

    test('drainAudio returns empty list', () async {
      expect(await AudioCodec.drainAudio(), isEmpty);
    });

    test('initDecode returns false', () async {
      expect(await AudioCodec.initDecode(), isFalse);
    });

    test('feedAac with non-empty bytes does nothing harmful', () async {
      await AudioCodec.feedAac(const []);
      await AudioCodec.feedAac(const [2, 0x12, 0x10, 0x56, 0xe5, 0x00]);
      await AudioCodec.feedAac(List.generate(256, (i) => i));
    });

    test('release completes without throwing', () async {
      await AudioCodec.release();
    });
  });
}