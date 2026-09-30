// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/messaging.dart';
import 'package:soshal_flutter/services/messaging_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

/// `flushMicrotasks` comes from `helpers/test_env.dart`.

void main() {
  final env = bootstrapTestEnv('test-messaging');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MessagingService', () {
    test('fetchPendingEphemeral parses every field of every row', () async {
      // The parser moved behind `runOffThreadCompute`, so this is what pins it
      // actually building `EphemeralMedia` objects: a `cast<EphemeralMedia>()`
      // over the raw decoded list returns a list of the right *type* and throws
      // only when a field is read.
      final msg = MessagingService();
      api.stubString(
        'crateFfiEphemeralEphemeralListPending',
        '[{"id":"em-1","message_id":"dm-1","conversation_id":"c-1",'
            '"conversation_type":"dm","media_url":"https://x/m.jpg",'
            '"media_type":"image","sender_pubkey":"pk-1",'
            '"recipient_pubkey":"pk-2","max_views":3,"current_views":1,'
            '"state":"pending","expires_at":1700000500,"created_at":1700000000,'
            '"viewed_at":0},'
            '{"id":"em-2","message_id":"dm-2","conversation_id":"c-2",'
            '"conversation_type":"dm","media_url":"https://x/n.png",'
            '"media_type":"image","sender_pubkey":"pk-2",'
            '"recipient_pubkey":"pk-1","max_views":1,"current_views":0,'
            '"state":"pending","expires_at":1700000600,"created_at":1700000100,'
            '"viewed_at":0}]',
      );

      final rows = await msg.fetchPendingEphemeral('pk-1');

      expect(rows, hasLength(2));
      expect(rows[0].id, 'em-1');
      expect(rows[0].messageId, 'dm-1');
      expect(rows[0].conversationId, 'c-1');
      expect(rows[0].mediaUrl, 'https://x/m.jpg');
      expect(rows[0].senderPubkey, 'pk-1');
      expect(rows[0].maxViews, 3);
      expect(rows[0].currentViews, 1);
      expect(rows[0].state, 'pending');
      expect(rows[0].expiresAt, 1700000500);
      expect(rows[1].id, 'em-2');
      expect(rows[1].recipientPubkey, 'pk-1');
      // The service exposes an unmodifiable view over the same rows.
      expect(msg.pendingEphemeral, hasLength(2));
      expect(msg.pendingEphemeral.first.id, 'em-1');
      expect(() => msg.pendingEphemeral.add(rows[0]), throwsUnsupportedError);

      final inv = api.callsOf('crateFfiEphemeralEphemeralListPending').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
    });

    test('fetchPendingEphemeral replaces, not appends, a second fetch',
        () async {
      final msg = MessagingService();
      api.stubString(
        'crateFfiEphemeralEphemeralListPending',
        '[{"id":"em-1","message_id":"dm-1","conversation_id":"c-1",'
            '"conversation_type":"dm","media_url":"u","media_type":"image",'
            '"sender_pubkey":"pk-1","recipient_pubkey":"pk-2","max_views":1,'
            '"current_views":0,"state":"pending","expires_at":1,'
            '"created_at":1,"viewed_at":0}]',
      );

      await msg.fetchPendingEphemeral('pk-1');
      expect(msg.pendingEphemeral, hasLength(1));

      await msg.fetchPendingEphemeral('pk-1');
      expect(msg.pendingEphemeral, hasLength(1),
          reason: 'the field is cleared before the new rows land');
    });

    test('insertLiveDm adds message to conversation', () async {
      final msg = MessagingService();
      var notified = 0;
      msg.addListener(() => notified++);
      api.stubBool('crateFfiMessagingMessagingStoreDm', true);

      final dm = DirectMessage(
        id: 'id1',
        sender: 'peer1',
        recipient: 'me',
        content: 'hello',
        createdAt: 1000,
        decrypted: true,
        isOwn: false,
      );

      msg.insertLiveDm(dm);

      expect(msg.conversations['peer1'], isNotNull);
      expect(msg.conversations['peer1']!.length, 1);
      expect(msg.conversations['peer1']!.first.content, 'hello');
      await flushMicrotasks();
      expect(notified, 1);
    });

    test('insertLiveDm ignores empty sender', () async {
      final msg = MessagingService();
      var notified = 0;
      msg.addListener(() => notified++);

      final dm = DirectMessage(
        id: 'id1',
        sender: '',
        recipient: 'me',
        content: 'hello',
        createdAt: 1000,
        decrypted: true,
        isOwn: false,
      );

      msg.insertLiveDm(dm);
      expect(msg.conversations.isEmpty, true);
      expect(notified, 0);
    });

    test('insertLiveDm deduplicates messages by id', () async {
      final msg = MessagingService();
      final dm1 = DirectMessage(
        id: 'id1',
        sender: 'peer1',
        recipient: 'me',
        content: 'hello',
        createdAt: 1000,
        decrypted: true,
        isOwn: false,
      );
      final dm2 = DirectMessage(
        id: 'id1',
        sender: 'peer1',
        recipient: 'me',
        content: 'hello2',
        createdAt: 1001,
        decrypted: true,
        isOwn: false,
      );

      msg.insertLiveDm(dm1);
      msg.insertLiveDm(dm2);

      expect(msg.conversations['peer1']!.length, 1);
      expect(msg.conversations['peer1']!.first.content, 'hello');
    });

    test('insertLiveDm limits conversation to 200 messages', () async {
      final msg = MessagingService();

      for (int i = 0; i < 210; i++) {
        final dm = DirectMessage(
          id: 'id$i',
          sender: 'peer1',
          recipient: 'me',
          content: 'msg $i',
          createdAt: 1000 + i,
          decrypted: true,
          isOwn: false,
        );
        msg.insertLiveDm(dm);
      }

      expect(msg.conversations['peer1']!.length, 200);
      expect(msg.conversations['peer1']!.first.content, 'msg 209');
      expect(msg.conversations['peer1']!.last.content, 'msg 10');
    });

    test('fetchDMs returns cached conversation', () async {
      final msg = MessagingService();
      final dm = DirectMessage(
        id: 'id1',
        sender: 'peer1',
        recipient: 'me',
        content: 'cached',
        createdAt: 1000,
        decrypted: true,
        isOwn: false,
      );

      msg.insertLiveDm(dm);

      final result = await msg.fetchDMs('peer1');
      expect(result.length, 1);
      expect(result.first.content, 'cached');
      expect(api.callsOf('crateFfiMessagingMessagingFetchDms').isEmpty, true);
    });

    test('fetchDMs fetches from backend when not cached', () async {
      final msg = MessagingService();
      var notified = 0;
      msg.addListener(() => notified++);

      const dmJson =
          '[{"id":"id1","sender":"peer1","recipient":"me","content":"hello","created_at":1000,"decrypted":true,"is_own":false}]';
      api.stubString('crateFfiMessagingMessagingFetchDms', dmJson);

      final result = await msg.fetchDMs('peer1');

      expect(result.length, 1);
      expect(result.first.content, 'hello');
      await flushMicrotasks();
      expect(notified, 1);
      final inv = api.callsOf('crateFfiMessagingMessagingFetchDms').single;
      expect(api.namedArg(inv, 'withPubkey'), 'peer1');
      expect(api.namedArg(inv, 'limit'), 100);
    });

    test('fetchDMs limits to 200 messages', () async {
      final msg = MessagingService();

      final dmList = List.generate(
        250,
        (i) => {
          'id': 'id$i',
          'sender': 'peer1',
          'recipient': 'me',
          'content': 'msg $i',
          'created_at': 1000 + i,
          'decrypted': true,
          'is_own': false,
        },
      );

      api.stubString(
        'crateFfiMessagingMessagingFetchDms',
        jsonEncode(dmList),
      );

      final result = await msg.fetchDMs('peer1');

      expect(result.length, 200);
      expect(result.first.content, 'msg 50');
      expect(result.last.content, 'msg 249');
    });

    test('fetchDMs sets error on exception', () async {
      final msg = MessagingService();
      api.stub('crateFfiMessagingMessagingFetchDms', (_) {
        throw Exception('Fetch failed');
      });

      await expectLater(
        msg.fetchDMs('peer1'),
        throwsException,
      );
      expect(msg.lastError, isNotNull);
    });

    test('fetchDMs(subscribe: false) hydrates without opening a live stream',
        () async {
      final msg = MessagingService();
      api.stubValue('crateFfiMessagingMessagingFetchDmsTyped', [
        DirectMessageDto(
          id: 'id1',
          sender: 'peer1',
          recipient: 'me',
          content: 'snippet',
          createdAt: BigInt.from(1000),
          decrypted: true,
          isOwn: false,
          tags: '[]',
        ),
      ]);

      final result = await msg.fetchDMs('peer1', limit: 1, subscribe: false);

      expect(result.single.content, 'snippet');
      expect(
        api.callsOf('crateFfiMessagingMessagingWatchDmsTyped').isEmpty,
        isTrue,
        reason: 'a bulk hydrate must not leave a per-conversation observer '
            'behind — every message write would wake it to decrypt again',
      );
    });

    test('fetchDMs subscribes by default (opened conversation stays live)',
        () async {
      final msg = MessagingService();
      final controller = StreamController<List<DirectMessageDto>>.broadcast();
      addTearDown(controller.close);
      api.stubStream(
          'crateFfiMessagingMessagingWatchDmsTyped', controller.stream);
      api.stubValue(
          'crateFfiMessagingMessagingFetchDmsTyped', <DirectMessageDto>[]);

      await msg.fetchDMs('peer1');

      expect(
        api.callsOf('crateFfiMessagingMessagingWatchDmsTyped').length,
        1,
        reason: 'the default path must keep the live subscription intact',
      );
    });

    test('fetchDMs(force: true) bypasses the cache early-return', () async {
      final msg = MessagingService();
      msg.insertLiveDm(DirectMessage(
        id: 'id1',
        sender: 'peer1',
        recipient: 'me',
        content: 'stale',
        createdAt: 1000,
        decrypted: true,
        isOwn: false,
      ));
      api.stubValue('crateFfiMessagingMessagingFetchDmsTyped', [
        DirectMessageDto(
          id: 'id2',
          sender: 'peer1',
          recipient: 'me',
          content: 'fresh',
          createdAt: BigInt.from(2000),
          decrypted: true,
          isOwn: false,
          tags: '[]',
        ),
      ]);

      // Without force the cached conversation satisfies `limit: 1`.
      expect((await msg.fetchDMs('peer1', limit: 1)).single.content, 'stale');

      final forced =
          await msg.fetchDMs('peer1', limit: 1, subscribe: false, force: true);

      expect(
        forced.single.content,
        'fresh',
        reason: 'the inbox re-hydrate must re-read the snippet that triggered '
            'it, or the live update can never land',
      );
    });

    test('sendDM sends and stores locally', () async {
      final msg = MessagingService();
      var notified = 0;
      msg.addListener(() => notified++);

      api.stubString('crateFfiMessagingMessagingSendDm', 'eventid123');

      final result = await msg.sendDM(
        'test msg',
        'recipient_pk',
        'sender_pk',
      );

      expect(result, 'eventid123');
      expect(msg.conversations['recipient_pk'], isNotNull);
      expect(msg.conversations['recipient_pk']!.length, 1);
      expect(msg.conversations['recipient_pk']!.first.content, 'test msg');
      expect(msg.conversations['recipient_pk']!.first.isOwn, true);
      await flushMicrotasks();
      expect(notified, 1);

      final inv = api.callsOf('crateFfiMessagingMessagingSendDm').single;
      expect(api.namedArg(inv, 'content'), 'test msg');
      expect(api.namedArg(inv, 'recipientPubkey'), 'recipient_pk');
    });

    test('sendDM to a fresh recipient at cache capacity never crashes',
        () async {
      final msg = MessagingService();
      api.stubString('crateFfiMessagingMessagingSendDm', 'eventid123');

      // Fill the conversation cache to capacity with recently-active keys so
      // the incoming (unknown, epoch-0) recipient is the guaranteed LRU
      // victim once eviction runs. Regression: eviction ran AFTER the
      // containsKey-guard inserted the key, then `!`-unwrapped a key that
      // could already have been evicted — null crash.
      for (var i = 0; i < 50; i++) {
        final pk = 'peer_$i';
        msg.conversations[pk] = [
          DirectMessage(
            id: 'id$i',
            sender: pk,
            recipient: 'me',
            content: 'x',
            createdAt: 1000,
            decrypted: true,
            isOwn: false,
          ),
        ];
        msg.markConversationCached(pk);
      }

      final result = await msg.sendDM('hello evict', 'recipient_new', 'sender');
      expect(result, 'eventid123');
      expect(msg.conversations['recipient_new'], isNotNull);
      expect(msg.conversations['recipient_new']!.first.content, 'hello evict');
      expect(msg.conversations.length, lessThanOrEqualTo(50));
    });

    test('sendDM sets error on exception', () async {
      final msg = MessagingService();
      api.stub('crateFfiMessagingMessagingSendDm', (_) {
        throw Exception('Send failed');
      });

      expect(
        () => msg.sendDM('text', 'recipient', 'sender'),
        throwsException,
      );
      expect(msg.lastError, isNotNull);
    });

    test('resolvePubkey accepts 64-char hex', () async {
      final msg = MessagingService();
      final hexPubkey = '0123456789abcdef' * 4;
      final result = msg.resolvePubkey(hexPubkey);
      expect(result, hexPubkey.toLowerCase());
    });

    test('resolvePubkey accepts npub', () async {
      final msg = MessagingService();
      const npub = 'npub1xyz';
      const hex = 'abc123';
      api.stubString('crateFfiAuthAuthNpubDecode', hex);

      final result = msg.resolvePubkey(npub);
      expect(result, hex);

      final inv = api.callsOf('crateFfiAuthAuthNpubDecode').single;
      expect(api.namedArg(inv, 'npub'), npub);
    });

    test('resolvePubkey throws on invalid input', () async {
      final msg = MessagingService();

      expect(
        () => msg.resolvePubkey('invalid'),
        throwsException,
      );
    });

    test('sendGroupDm creates sorted deterministic group', () async {
      final msg = MessagingService();
      const groupId = 'eventid456';
      api.stubString('crateFfiMessagingMessagingSendGroupDm', groupId);

      final result = await msg.sendGroupDm(
        content: 'group msg',
        participantPubkeys: ['pk3', 'pk1', 'pk2'],
      );

      expect(result, groupId);
      final inv = api.callsOf('crateFfiMessagingMessagingSendGroupDm').single;
      expect(api.namedArg(inv, 'content'), 'group msg');
      final groupIdArg = api.namedArg(inv, 'groupId') as String;
      expect(groupIdArg, 'pk1,pk2,pk3');
    });

    test('pendingEphemeral returns immutable list', () async {
      final msg = MessagingService();
      final pending = msg.pendingEphemeral;
      expect(pending, isA<List>());
      expect(pending.isEmpty, true);
    });

    test('watchDMs parses incoming json stream into list of DirectMessage',
        () async {
      final msg = MessagingService();
      final controller = StreamController<String>();
      addTearDown(controller.close);

      api.stub('crateFfiMessagingMessagingWatchDms', (_) => controller.stream);

      final stream = msg.watchDMs('peer_reactive');
      final emissions = <List<DirectMessage>>[];
      final sub = stream.listen(emissions.add);
      addTearDown(sub.cancel);

      const payload1 =
          '[{"id":"dm_r1","sender":"peer_reactive","recipient":"me","content":"reactive msg 1","created_at":2000,"decrypted":true,"is_own":false}]';
      controller.add(payload1);
      await pumpEventQueue();

      expect(emissions.length, 1);
      expect(emissions[0].length, 1);
      expect(emissions[0][0].id, 'dm_r1');
      expect(emissions[0][0].content, 'reactive msg 1');

      const payload2 =
          '[{"id":"dm_r2","sender":"me","recipient":"peer_reactive","content":"reply","created_at":2001,"decrypted":true,"is_own":true},'
          '{"id":"dm_r1","sender":"peer_reactive","recipient":"me","content":"reactive msg 1","created_at":2000,"decrypted":true,"is_own":false}]';
      controller.add(payload2);
      await pumpEventQueue();

      expect(emissions.length, 2);
      expect(emissions[1].length, 2);
      expect(emissions[1][0].id, 'dm_r2');
    });

    test(
        'subscribeToConversation updates conversations map and notifies listeners on stream events',
        () async {
      final msg = MessagingService();
      final controller = StreamController<String>();
      addTearDown(controller.close);

      api.stub('crateFfiMessagingMessagingWatchDms', (_) => controller.stream);

      var notified = 0;
      msg.addListener(() => notified++);

      msg.subscribeToConversation('peer_reactive_2');

      const payload =
          '[{"id":"msg_live","sender":"peer_reactive_2","recipient":"me","content":"stream hello","created_at":3000,"decrypted":true,"is_own":false}]';
      controller.add(payload);
      await pumpEventQueue();
      await flushMicrotasks();

      expect(msg.conversations['peer_reactive_2'], isNotNull);
      expect(
          msg.conversations['peer_reactive_2']!.first.content, 'stream hello');
      expect(notified, greaterThanOrEqualTo(1));

      msg.resetForAccountSwitch();
      // Should cancel active subscriptions
      controller.add('[]');
      await pumpEventQueue();
      expect(msg.conversations.isEmpty, true);
    });

    test('watchConversations parses list of peer pubkeys from stream',
        () async {
      final msg = MessagingService();
      final controller = StreamController<List<String>>();
      addTearDown(controller.close);

      api.stub('crateFfiMessagingMessagingWatchConversations',
          (_) => controller.stream);

      final stream = msg.watchConversations('me');
      final emissions = <List<String>>[];
      final sub = stream.listen(emissions.add);
      addTearDown(sub.cancel);

      controller.add(['peer_a', 'peer_b']);
      await pumpEventQueue();

      expect(emissions.length, 1);
      expect(emissions[0], ['peer_a', 'peer_b']);
    });

    test('watchDMs handles native DirectMessageDto typed streams directly',
        () async {
      final msg = MessagingService();
      final controller = StreamController<List<DirectMessageDto>>();
      addTearDown(controller.close);

      api.stubStream(
          'crateFfiMessagingMessagingWatchDmsTyped', controller.stream);

      final stream = msg.watchDMs('peer_typed');
      final emissions = <List<DirectMessage>>[];
      final sub = stream.listen(emissions.add);
      addTearDown(sub.cancel);

      controller.add([
        DirectMessageDto(
          id: 'dm_1',
          sender: 'peer_typed',
          recipient: 'me',
          content: 'hello via typed binary sse stream',
          createdAt: BigInt.from(54321),
          decrypted: true,
          isOwn: false,
          tags: '[]',
        ),
      ]);
      await pumpEventQueue();

      expect(emissions.length, 1);
      final message = emissions[0].single;
      expect(message.id, 'dm_1');
      expect(message.sender, 'peer_typed');
      expect(message.content, 'hello via typed binary sse stream');
      expect(message.createdAt, 54321);
      expect(message.decrypted, true);
      expect(message.isOwn, false);
      expect(msg.conversations['peer_typed']?.length, 1);
    });

    test('fetchDMs handles native DirectMessageDto typed results directly',
        () async {
      final msg = MessagingService();
      api.stubValue('crateFfiMessagingMessagingFetchDmsTyped', [
        DirectMessageDto(
          id: 'dm_fetch_1',
          sender: 'me',
          recipient: 'peer_fetch',
          content: 'fetched typed dm',
          createdAt: BigInt.from(9999),
          decrypted: true,
          isOwn: true,
          tags: '[]',
        ),
      ]);

      final messages = await msg.fetchDMs('peer_fetch');
      expect(messages.length, 1);
      expect(messages.first.id, 'dm_fetch_1');
      expect(messages.first.content, 'fetched typed dm');
      expect(messages.first.isOwn, true);
    });
  });

  group('IdentityService', () {
    test('watchProfile parses JSON profile from stream', () async {
      final identity = IdentityService();
      final controller = StreamController<String>();
      addTearDown(controller.close);

      api.stub(
          'crateFfiIdentityIdentityWatchProfile', (_) => controller.stream);

      final stream = identity.watchProfile('pk_alice');
      final emissions = <ProfileInfo>[];
      final sub = stream.listen(emissions.add);
      addTearDown(sub.cancel);

      final payload = jsonEncode({
        'pubkey': 'pk_alice',
        'name': 'Alice',
        'display_name': 'Alice in Wonderland',
        'picture': 'https://example.com/alice.png',
        'banner': '',
        'about': 'Curiouser and curiouser',
        'nip05': 'alice@soshal.net',
        'nip05_valid': true,
        'created_at': 1700000000,
        'followers': 42,
        'following': 10,
        'is_following': false,
        'wot_status': 'verified',
      });
      controller.add(payload);
      await pumpEventQueue();

      expect(emissions.length, 1);
      expect(emissions[0].pubkey, 'pk_alice');
      expect(emissions[0].name, 'Alice');
      expect(emissions[0].displayName, 'Alice in Wonderland');
      expect(emissions[0].followers, 42);
    });

    test(
        'subscribeToProfile updates profiles cache and notifies listeners on stream updates',
        () async {
      final identity = IdentityService();
      final controller = StreamController<String>();
      addTearDown(controller.close);

      api.stub(
          'crateFfiIdentityIdentityWatchProfile', (_) => controller.stream);

      var notified = 0;
      identity.addListener(() => notified++);

      identity.subscribeToProfile('pk_bob');

      final payload1 = jsonEncode({
        'pubkey': 'pk_bob',
        'name': 'Bob',
        'display_name': 'Builder Bob',
        'picture': '',
        'banner': '',
        'about': '',
        'nip05': '',
        'nip05_valid': false,
        'created_at': 1000,
        'followers': 1,
        'following': 0,
        'is_following': false,
        'wot_status': 'unknown',
      });
      controller.add(payload1);
      await pumpEventQueue();
      await flushMicrotasks();

      expect(identity.profiles['pk_bob'], isNotNull);
      expect(identity.profiles['pk_bob']!.name, 'Bob');
      expect(notified, greaterThanOrEqualTo(1));

      // Stream a follow update
      final payload2 = jsonEncode({
        'pubkey': 'pk_bob',
        'name': 'Bob',
        'display_name': 'Builder Bob',
        'picture': '',
        'banner': '',
        'about': '',
        'nip05': '',
        'nip05_valid': false,
        'created_at': 1000,
        'followers': 2,
        'following': 0,
        'is_following': true,
        'wot_status': 'unknown',
      });
      controller.add(payload2);
      await pumpEventQueue();
      await flushMicrotasks();

      expect(identity.profiles['pk_bob']!.followers, 2);
      expect(identity.profiles['pk_bob']!.isFollowing, true);

      // Verify resetForAccountSwitch unsubscribes and clears
      identity.resetForAccountSwitch();
      expect(identity.profiles.isEmpty, true);

      controller.add(payload1);
      await pumpEventQueue();
      // Should not repopulate since subscription was cancelled
      expect(identity.profiles.isEmpty, true);
    });

    String profileJson(String pubkey, {String name = '', int followers = 0}) =>
        jsonEncode({
          'pubkey': pubkey,
          'name': name,
          'display_name': '',
          'picture': '',
          'banner': '',
          'about': '',
          'nip05': '',
          'nip05_valid': false,
          'created_at': 0,
          'followers': followers,
          'following': 0,
          'is_following': false,
          'wot_status': 'unknown',
        });

    test('getProfiles fetches every pubkey in one call, in request order',
        () async {
      final identity = IdentityService();
      api.stub('crateFfiIdentityIdentityWatchProfile',
          (_) => const Stream<String>.empty());
      api.stub('crateFfiIdentityIdentityGetProfilesBatch', (inv) {
        final pks = api.namedArg(inv, 'pubkeys') as List<dynamic>;
        return jsonEncode([
          for (final p in pks)
            jsonDecode(profileJson(p as String, name: 'n-$p', followers: 7)),
        ]);
      });

      final out = await identity.getProfiles(['pk_a', 'pk_b', 'pk_c']);

      expect(api.callCount('crateFfiIdentityIdentityGetProfilesBatch'), 1);
      expect(api.callCount('crateFfiIdentityIdentityGetProfile'), 0);
      expect(out.map((p) => p.pubkey), ['pk_a', 'pk_b', 'pk_c']);
      expect(out.first.name, 'n-pk_a');
      expect(out.first.followers, 7);
    });

    test('getProfiles skips blanks and duplicates but keeps first-seen order',
        () async {
      final identity = IdentityService();
      api.stub('crateFfiIdentityIdentityWatchProfile',
          (_) => const Stream<String>.empty());
      api.stub('crateFfiIdentityIdentityGetProfilesBatch', (inv) {
        final pks = api.namedArg(inv, 'pubkeys') as List<dynamic>;
        return jsonEncode([
          for (final p in pks) jsonDecode(profileJson(p as String)),
        ]);
      });

      final out =
          await identity.getProfiles(['pk_a', '  ', 'pk_b', 'pk_a', '']);

      final inv =
          api.callsOf('crateFfiIdentityIdentityGetProfilesBatch').single;
      expect(api.namedArg(inv, 'pubkeys'), ['pk_a', 'pk_b']);
      expect(out.map((p) => p.pubkey), ['pk_a', 'pk_b']);
    });

    test('getProfiles serves the second call from cache', () async {
      final identity = IdentityService();
      api.stub('crateFfiIdentityIdentityWatchProfile',
          (_) => const Stream<String>.empty());
      api.stub('crateFfiIdentityIdentityGetProfilesBatch', (inv) {
        final pks = api.namedArg(inv, 'pubkeys') as List<dynamic>;
        return jsonEncode([
          for (final p in pks) jsonDecode(profileJson(p as String)),
        ]);
      });

      await identity.getProfiles(['pk_a', 'pk_b']);
      final out = await identity.getProfiles(['pk_a', 'pk_b']);

      expect(api.callCount('crateFfiIdentityIdentityGetProfilesBatch'), 1);
      expect(out.map((p) => p.pubkey), ['pk_a', 'pk_b']);
    });

    test('getProfiles refresh re-requests and merges non-empty cached fields',
        () async {
      final identity = IdentityService();
      api.stub('crateFfiIdentityIdentityWatchProfile',
          (_) => const Stream<String>.empty());
      api.stub('crateFfiIdentityIdentityGetProfilesBatch', (inv) {
        final pks = api.namedArg(inv, 'pubkeys') as List<dynamic>;
        return jsonEncode([
          for (final p in pks)
            // Second response omits the name, so the cached one must survive.
            jsonDecode(
              profileJson(p as String, name: p == 'pk_a' ? 'Alice' : ''),
            ),
        ]);
      });

      await identity.getProfiles(['pk_a']);
      final out = await identity.getProfiles(['pk_a'], refresh: true);

      expect(api.callCount('crateFfiIdentityIdentityGetProfilesBatch'), 2);
      expect(out.single.name, 'Alice');
    });

    test('getProfiles omits pubkeys the batch did not return', () async {
      final identity = IdentityService();
      api.stub('crateFfiIdentityIdentityWatchProfile',
          (_) => const Stream<String>.empty());
      // Batch answers for pk_a only — pk_b has no stored row upstream.
      api.stub('crateFfiIdentityIdentityGetProfilesBatch',
          (inv) => jsonEncode([jsonDecode(profileJson('pk_a'))]));

      final out = await identity.getProfiles(['pk_a', 'pk_b']);

      expect(out.map((p) => p.pubkey), ['pk_a']);
    });

    test('getProfiles surfaces a bridge failure and returns nothing', () async {
      final identity = IdentityService();
      api.stub('crateFfiIdentityIdentityWatchProfile',
          (_) => const Stream<String>.empty());
      api.stub('crateFfiIdentityIdentityGetProfilesBatch',
          (_) => throw Exception('db down'));

      final out = await identity.getProfiles(['pk_a', 'pk_b']);

      expect(out, isEmpty);
      expect(identity.lastError.toString(), contains('db down'));
    });
  });
}
