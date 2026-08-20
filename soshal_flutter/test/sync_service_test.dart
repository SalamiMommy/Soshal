// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/p2p_service.dart';
import 'package:soshal_flutter/services/sync_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-sync');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubBool('crateFfiFeedFeedValidateNote', true);
  });

  group('SyncService', () {
    test('start with empty relays is a no-op', () async {
      final sync = SyncService();
      await sync.start(relays: const []);
      expect(api.calls, isEmpty);
      expect(sync.started, isFalse);
    });

    test('start calls engine once and is idempotent', () async {
      final sync = SyncService();
      api.stub('crateFfiSyncSyncEvents', (_) => Stream<String>.empty());
      api.stubString('crateFfiSyncSyncStart', 'ok');

      await sync.start(relays: const ['wss://relay.example']);
      await sync.start(relays: const ['wss://relay.example']);
      expect(sync.started, isTrue);
      expect(api.callCount('crateFfiSyncSyncStart'), 1);
      final inv = api.callsOf('crateFfiSyncSyncStart').single;
      expect(api.namedArg(inv, 'relaysJson'), contains('relay.example'));
    });

    test('start failure clears started flag and sets lastError', () async {
      final sync = SyncService();
      api.stub('crateFfiSyncSyncEvents', (_) => Stream<String>.empty());
      api.stub('crateFfiSyncSyncStart', (_) => throw Exception('boom'));

      await sync.start(relays: const ['wss://relay.example']);
      expect(sync.started, isFalse);
      expect(sync.lastError, contains('boom'));
    });

    test('routes stream events into feed and messaging', () async {
      final sync = SyncService();
      final feed = FeedService();
      final messaging = MessagingService();
      sync.attach(feed: feed, messaging: messaging, p2p: P2pService());

      final controller = StreamController<String>.broadcast();
      api.stub('crateFfiSyncSyncEvents', (_) => controller.stream);
      api.stubString('crateFfiSyncSyncStart', 'ok');
      await sync.start(relays: const ['wss://relay.example']);

      controller.add('{"t":"feed","id":"ev-1","pubkey":"pk-1",'
          '"content":"hello","created_at":1700000000}');
      controller.add('{"t":"dm","id":"dm-1","sender":"peer-1",'
          '"content":"secret","created_at":1}');
      await pumpEventQueue();

      expect(feed.posts.single.content, 'hello');
      expect(messaging.conversations['peer-1']!.single.content, 'secret');

      feed.insertLivePost(FeedPost(
        eventId: 'ev-2',
        pubkey: 'pk-9',
        content: 'seed',
        createdAt: 2,
        reactions: 0,
        replies: 0,
        reposts: 0,
        liked: false,
      ));
      controller.add('{"t":"reaction","event_id":"ev-2","pubkey":"pk-2",'
          '"content":"+"}');
      controller.add('{"t":"profile","pubkey":"pk-1"}');
      controller.add('not json');
      await pumpEventQueue();

      final reacted =
          feed.posts.firstWhere((p) => p.eventId == 'ev-2');
      expect(reacted.reactions, 1);
      expect(reacted.liked, isTrue);
      expect(sync.lastError, isNotNull);
      await controller.close();
    });

    test('stop calls engine stop and clears started flag', () async {
      final sync = SyncService();
      api.stub('crateFfiSyncSyncEvents', (_) => Stream<String>.empty());
      api.stubString('crateFfiSyncSyncStart', 'ok');
      api.stubBool('crateFfiSyncSyncStop', true);

      await sync.start(relays: const ['wss://relay.example']);
      await sync.stop();
      expect(sync.started, isFalse);
      expect(api.callCount('crateFfiSyncSyncStop'), 1);

      await sync.stop();
      expect(api.callCount('crateFfiSyncSyncStop'), 1,
          reason: 'stop is idempotent');
    });
  });
}