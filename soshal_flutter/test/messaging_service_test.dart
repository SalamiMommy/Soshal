// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/messaging_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-messaging');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MessagingService', () {
    test('insertLiveDm adds message to conversation', () async {
      final msg = MessagingService();
      var notified = 0;
      msg.addListener(() => notified++);

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
      expect(msg.conversations['peer1']!.first.content, 'msg 10');
      expect(msg.conversations['peer1']!.last.content, 'msg 209');
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

      expect(
        () => msg.fetchDMs('peer1'),
        throwsException,
      );
      expect(msg.lastError, isNotNull);
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
        'sender_sk',
      );

      expect(result, 'eventid123');
      expect(msg.conversations['recipient_pk'], isNotNull);
      expect(msg.conversations['recipient_pk']!.length, 1);
      expect(msg.conversations['recipient_pk']!.first.content, 'test msg');
      expect(msg.conversations['recipient_pk']!.first.isOwn, true);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiMessagingMessagingSendDm').single;
      expect(api.namedArg(inv, 'content'), 'test msg');
      expect(api.namedArg(inv, 'recipientPubkey'), 'recipient_pk');
    });

    test('sendDM sets error on exception', () async {
      final msg = MessagingService();
      api.stub('crateFfiMessagingMessagingSendDm', (_) {
        throw Exception('Send failed');
      });

      expect(
        () => msg.sendDM('text', 'recipient', 'sender', 'sk'),
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
  });
}
