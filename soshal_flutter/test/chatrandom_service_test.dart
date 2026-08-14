// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/chatrandom_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-chatrandom');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ChatrandomService', () {
    test('availableContent builds JSON string', () {
      api.stubString(
        'crateFfiChatrandomChatrandomAvailableContent',
        '{"interests":["music","games"],"media_type":"text","mode":"voice"}',
      );

      final svc = ChatrandomService();
      final result = svc.availableContent(
        interests: ['music', 'games'],
        mediaType: 'text',
        mode: 'voice',
      );

      expect(result, contains('interests'));
      expect(result, contains('media_type'));
      final inv = api.callsOf('crateFfiChatrandomChatrandomAvailableContent').single;
      expect(api.namedArg(inv, 'interests'), ['music', 'games']);
      expect(api.namedArg(inv, 'mediaType'), 'text');
      expect(api.namedArg(inv, 'mode'), 'voice');
    });

    test('send publishes chatrandom request', () async {
      api.stubString('crateFfiChatrandomChatrandomSend', 'event_id_123');

      final svc = ChatrandomService();
      var notified = 0;
      svc.addListener(() => notified++);

      final result = await svc.send(
        requestType: 'request',
        peers: ['pk1', 'pk2'],
        contentJson: '{"interests":["music"]}',
      );

      expect(result, 'event_id_123');
      expect(notified, 0);
      expect(svc.lastError, isNull);
      final inv = api.callsOf('crateFfiChatrandomChatrandomSend').single;
      expect(api.namedArg(inv, 'requestType'), 'request');
      expect(api.namedArg(inv, 'peers'), ['pk1', 'pk2']);
    });

    test('send sets error on exception', () async {
      final svc = ChatrandomService();
      api.stub('crateFfiChatrandomChatrandomSend', (_) {
        throw Exception('Send failed');
      });

      expect(
        () => svc.send(
          requestType: 'request',
          peers: ['pk1'],
          contentJson: '{}',
        ),
        throwsException,
      );
      expect(svc.lastError, isNotNull);
    });

    test('fetch loads peer events and notifies', () async {
      final svc = ChatrandomService();
      var notified = 0;
      svc.addListener(() => notified++);

      const peerJson =
          '[{"id":"e1","pubkey":"pk1","content":"availability","created_at":1000}]';
      api.stubString('crateFfiChatrandomChatrandomFetch', peerJson);

      final result = await svc.fetch('mypk', limit: 20);

      expect(result.length, 1);
      expect(result.first.pubkey, 'pk1');
      expect(svc.peers.length, 1);
      expect(notified, 1);
      expect(svc.lastError, isNull);

      final inv = api.callsOf('crateFfiChatrandomChatrandomFetch').single;
      expect(api.namedArg(inv, 'myPubkey'), 'mypk');
      expect(api.namedArg(inv, 'limit'), BigInt.from(20));
    });

    test('fetch with author filters by peer announcements', () async {
      final svc = ChatrandomService();
      const peerJson =
          '[{"id":"e1","pubkey":"peer_pk","content":"{}","created_at":1000}]';
      api.stubString('crateFfiChatrandomChatrandomFetch', peerJson);

      await svc.fetch('mypk', author: 'peer_pk', limit: 10);

      final inv = api.callsOf('crateFfiChatrandomChatrandomFetch').single;
      expect(api.namedArg(inv, 'author'), 'peer_pk');
    });

    test('fetch sets error on exception', () async {
      final svc = ChatrandomService();
      api.stub('crateFfiChatrandomChatrandomFetch', (_) {
        throw Exception('Fetch failed');
      });

      expect(
        () => svc.fetch('mypk'),
        throwsException,
      );
      expect(svc.lastError, isNotNull);
      expect(svc.peers.isEmpty, true);
    });

    test('ChatrandomPeer parses JSON correctly', () {
      final json = <String, dynamic>{
        'id': 'e123',
        'pubkey': 'pk_abc',
        'content': '{"interests":["music"]}',
        'created_at': 1700000000,
      };
      final peer = ChatrandomPeer.fromJson(json);
      expect(peer.id, 'e123');
      expect(peer.pubkey, 'pk_abc');
      expect(peer.content, contains('interests'));
      expect(peer.createdAt, 1700000000);
    });

    test('ChatrandomPeer handles missing fields', () {
      final json = <String, dynamic>{};
      final peer = ChatrandomPeer.fromJson(json);
      expect(peer.id, '');
      expect(peer.pubkey, '');
      expect(peer.content, '');
      expect(peer.createdAt, 0);
    });
  });
}
