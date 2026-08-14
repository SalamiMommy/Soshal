// ignore_for_file: invalid_use_of_internal_member
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/streaming_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-streaming');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('StreamingService', () {
    test('nextMoqGroupSeq is a monotonic local counter', () {
      final streaming = StreamingService();
      expect(streaming.nextMoqGroupSeq(), 0);
      expect(streaming.nextMoqGroupSeq(), 1);
      expect(streaming.nextMoqGroupSeq(), 2);
      expect(api.calls, isEmpty);
    });

    test('broadcast lifecycle publishes control groups and resets seq',
        () async {
      final streaming = StreamingService();
      api.stub(
          'crateFfiP2PP2PMoqEncodeGroup', (_) => Uint8List.fromList([1, 2, 3]));
      api.stubString(
          'crateFfiP2PP2PMoqPublishGroup', '{"status":"ok","groups":1}');

      streaming.nextMoqGroupSeq();
      await streaming.startMoqBroadcast(streamId: 's-1', title: 'hi');
      expect(streaming.isBroadcasting, isTrue);
      expect(streaming.activeMoqStreamId, 's-1');
      expect(streaming.nextMoqGroupSeq(), 0, reason: 'broadcast resets seq');

      final pubInv = api.callsOf('crateFfiP2PP2PMoqPublishGroup').single;
      expect(api.namedArg(pubInv, 'streamId'), 's-1');

      await streaming.stopMoqBroadcast();
      expect(streaming.isBroadcasting, isFalse);
      expect(streaming.activeMoqStreamId, isNull);
      expect(api.callCount('crateFfiP2PP2PMoqPublishGroup'), 2);
    });

    test('subscribeMoqStream passes args and clears error', () async {
      final streaming = StreamingService();
      api.stubString(
          'crateFfiStreamingStreamingMoqSubscribeStream', 'subscribed');

      final status = await streaming.subscribeMoqStream(
          streamId: 's-1', subscriberPubkey: 'pk-1');
      expect(status, 'subscribed');
      expect(streaming.lastError, isNull);
      final inv =
          api.callsOf('crateFfiStreamingStreamingMoqSubscribeStream').single;
      expect(api.namedArg(inv, 'streamId'), 's-1');
      expect(api.namedArg(inv, 'subscriberPubkey'), 'pk-1');
    });

    test('FFI throw sets lastError and rethrows', () async {
      final streaming = StreamingService();
      api.stub('crateFfiStreamingStreamingMoqSubscribeStream',
          (_) => throw Exception('moq down'));

      await expectLater(
          streaming.subscribeMoqStream(
              streamId: 's-1', subscriberPubkey: 'pk-1'),
          throwsException);
      expect(streaming.lastError, contains('moq down'));
    });

    test('storyReact swallows errors and returns false', () async {
      final streaming = StreamingService();
      api.stub('crateFfiStreamingStreamingStoryReact',
          (_) => throw Exception('story down'));

      expect(await streaming.storyReact('st-1', 'pk-1', 'like'), isFalse);
      expect(streaming.lastError, contains('story down'));
    });

    test('initLocalVideoServer stores the returned port', () async {
      final streaming = StreamingService();
      api.stubInt('crateFfiStreamingStreamingStartLocalServer', 8787);

      expect(await streaming.initLocalVideoServer(), 8787);
      expect(streaming.localVideoServerPort, 8787);
    });

    test('group builders encode track metadata without FFI', () {
      final streaming = StreamingService();
      final v = streaming.buildVideoGroup(
          groupSeq: 1, timestampMs: 42, jpeg: [1, 2]);
      expect((v['objects'] as List).single['header']['track_id'], 0);

      final h = streaming.buildH264Group(
          groupSeq: 1, timestampMs: 42, nal: [3], keyframe: false);
      final hHeader = (h['objects'] as List).single['header'];
      expect(hHeader['track_type'], 'VideoDelta');
      expect(hHeader['track_id'], 1);

      final a = streaming.buildAudioGroup(
          groupSeq: 2, timestampMs: 43, aac: [4], config: true);
      final aHeader = (a['objects'] as List).single['header'];
      expect(aHeader['track_type'], 'AudioDatagram');
      expect(aHeader['track_id'], 2);
      expect(api.calls, isEmpty);
    });
  });
}