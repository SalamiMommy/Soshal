// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/calls_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-calls');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('CallsService signals', () {
    test('sendSignal returns event id, forwards args, notifies', () async {
      final calls = CallsService();
      var notifications = 0;
      calls.addListener(() => notifications++);
      api.stub('crateFfiCallsCallsSendSignal', (_) async => 'ev-1');

      final eventId = await calls.sendSignal(
        signalType: 'offer',
        targetPubkey: 'peer-1',
        callId: 'c-1',
        sdp: 'v=0',
        candidate: 'cand-1',
        mediaType: 'audio',
      );

      expect(eventId, 'ev-1');
      expect(calls.lastError, isNull);
      await pumpEventQueue();
      expect(notifications, 1);
      final inv = api.callsOf('crateFfiCallsCallsSendSignal').single;
      expect(api.namedArg(inv, 'signalType'), 'offer');
      expect(api.namedArg(inv, 'targetPubkey'), 'peer-1');
      expect(api.namedArg(inv, 'callId'), 'c-1');
      expect(api.namedArg(inv, 'sdp'), 'v=0');
      expect(api.namedArg(inv, 'candidate'), 'cand-1');
      expect(api.namedArg(inv, 'mediaType'), 'audio');
    });

    test('sendSignal error rethrows and records lastError', () async {
      final calls = CallsService();
      api.stub('crateFfiCallsCallsSendSignal',
          (_) async => throw Exception('signal boom'));
      await expectLater(
        calls.sendSignal(
          signalType: 'offer',
          targetPubkey: 'peer-1',
          callId: 'c-1',
        ),
        throwsA(anything),
      );
      expect(calls.lastError, contains('signal boom'));
    });

    test('fetchSignals parses content-embedded call metadata', () async {
      final calls = CallsService();
      api.stub(
        'crateFfiCallsCallsFetchSignals',
        (_) async =>
            '[{"id":"s1","pubkey":"peer-1",'
        '"content":"{\\"call_id\\":\\"c-1\\",\\"type\\":\\"answer\\",'
        '\\"media_type\\":\\"video\\"}","created_at":1700000000,"kind":20002,'
        '"p_tags":["peer-1","peer-2"]}]',
      );

      final signals = await calls.fetchSignals('me');

      final signal = signals.single;
      expect(signal.id, 's1');
      expect(signal.pubkey, 'peer-1');
      expect(signal.callId, 'c-1');
      expect(signal.signalType, 'answer');
      expect(signal.signalMediaType, 'video');
      expect(signal.kind, 20002);
      expect(signal.pTags, ['peer-1', 'peer-2']);
      expect(calls.signals, hasLength(1));
      expect(signals, hasLength(1));
      expect(calls.lastError, isNull);

      final inv = api.callsOf('crateFfiCallsCallsFetchSignals').single;
      expect(api.namedArg(inv, 'myPubkey'), 'me');
    });

    test('fetchSignals empty list clears stored signals', () async {
      final calls = CallsService();
      api.stub('crateFfiCallsCallsFetchSignals', (_) async => '[]');
      expect(await calls.fetchSignals('me'), isEmpty);
      expect(calls.signals, isEmpty);
    });

    test('fetchSignals error rethrows, keeps previous signals', () async {
      final calls = CallsService();
      api.stub(
        'crateFfiCallsCallsFetchSignals',
        (_) async =>
            '[{"id":"s1","pubkey":"p","content":"{\\"call_id\\":\\"c\\",'
        '\\"type\\":\\"offer\\"}","created_at":1,"kind":20001,"p_tags":[]}]',
      );
      await calls.fetchSignals('me');

      api.stub('crateFfiCallsCallsFetchSignals',
          (_) async => throw Exception('fetch boom'));
      await expectLater(calls.fetchSignals('me'), throwsA(anything));
      expect(calls.lastError, contains('fetch boom'));
      expect(calls.signals.single.id, 's1');
    });

    test('sanitizeSdp returns redacted sdp, error rethrows', () {
      final calls = CallsService();
      api.stubString(
          'crateFfiWebrtcWebrtcSanitizeSdp', 'v=0 redacted');
      expect(calls.sanitizeSdp('v=0 192.168.1.5', forceRelay: true),
          'v=0 redacted');
      final inv = api.callsOf('crateFfiWebrtcWebrtcSanitizeSdp').single;
      expect(api.namedArg(inv, 'forceRelay'), isTrue);

      api.stub('crateFfiWebrtcWebrtcSanitizeSdp',
          (_) => throw Exception('sdp boom'));
      expect(() => calls.sanitizeSdp('v=0'), throwsA(anything));
      expect(calls.lastError, contains('sdp boom'));
    });

    test('iceConfig returns config, forwards privacy level', () {
      final calls = CallsService();
      api.stubString(
          'crateFfiWebrtcWebrtcGetIceConfig', '{"iceServers":[]}');
      expect(calls.iceConfig('strict'), '{"iceServers":[]}');
      final inv = api.callsOf('crateFfiWebrtcWebrtcGetIceConfig').single;
      expect(api.namedArg(inv, 'privacyLevel'), 'strict');
    });
  });

  group('CallsService call state', () {
    test('startCall sets state and ticks elapsed timer', () async {
      final calls = CallsService();
      var notifications = 0;
      calls.addListener(() => notifications++);

      calls.startCall(callId: 'c-1', peer: 'peer-1', mediaType: 'audio');

      expect(calls.callId, 'c-1');
      expect(calls.peer, 'peer-1');
      expect(calls.mediaType, 'audio');
      expect(calls.inCall, isTrue);
      expect(calls.elapsed.inSeconds, 0);
      await pumpEventQueue();
      expect(notifications, 1);

      await Future<void>.delayed(const Duration(milliseconds: 1200));
      expect(calls.elapsed.inSeconds, greaterThanOrEqualTo(1));
      expect(notifications, greaterThan(1),
          reason: 'periodic timer notifies listeners');
    });

    test('endCall clears state and stops timer', () async {
      final calls = CallsService();
      var notifications = 0;
      calls.addListener(() => notifications++);
      calls.startCall(callId: 'c-1', peer: 'peer-1', mediaType: 'audio');

      calls.endCall();
      expect(calls.inCall, isFalse);
      expect(calls.elapsed, Duration.zero);
      expect(calls.callId, isNull);
      expect(calls.peer, isNull);
      expect(calls.mediaType, isNull);

      await pumpEventQueue();
      final afterEnd = notifications;
      await Future<void>.delayed(const Duration(milliseconds: 1200));
      expect(notifications, afterEnd,
          reason: 'timer cancelled after endCall');
    });

    test('clearLastError clears lastError', () async {
      final calls = CallsService();
      api.stub('crateFfiCallsCallsSendSignal',
          (_) => throw Exception('boom'));
      await expectLater(
        calls.sendSignal(
          signalType: 'end',
          targetPubkey: 'peer-1',
          callId: 'c-1',
        ),
        throwsA(anything),
      );
      expect(calls.lastError, contains('boom'));
      calls.clearLastError();
      expect(calls.lastError, isNull);
    });
  });
}