// ignore_for_file: invalid_use_of_internal_member
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/h264_codec.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-h264-codec');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('H264Codec off-Android', () {
    test('isSupported returns false without touching the channel', () async {
      expect(await H264Codec.isSupported(), isFalse);
    });

    test('initEncode returns false with required dimensions', () async {
      expect(await H264Codec.initEncode(width: 640, height: 480), isFalse);
      expect(await H264Codec.initEncode(
        width: 1280,
        height: 720,
        bitrate: 1000000,
        fps: 30,
      ), isFalse);
    });

    test('feedEncode returns empty for empty and non-empty frames', () async {
      expect(await H264Codec.feedEncode(Uint8List(0)), isEmpty);
      final frame = Uint8List.fromList(List.filled(640 * 480 * 4, 7));
      expect(await H264Codec.feedEncode(frame), isEmpty,
          reason: 'non-empty frame must not crash or reach FFI');
    });

    test('initDecode returns false', () async {
      expect(await H264Codec.initDecode(), isFalse);
    });

    test('feedDecode returns empty for empty and non-empty NAL', () async {
      expect(await H264Codec.feedDecode(Uint8List(0)), isEmpty);
      final nal = Uint8List.fromList([0, 0, 0, 1, 0x65, 1, 2, 3]);
      expect(await H264Codec.feedDecode(nal), isEmpty);
    });

    test('initRecord returns null', () async {
      expect(await H264Codec.initRecord(), isNull);
    });

    test('stopRecord returns null', () async {
      expect(await H264Codec.stopRecord(), isNull);
    });

    test('release completes without throwing', () async {
      await H264Codec.release();
    });
  });
}