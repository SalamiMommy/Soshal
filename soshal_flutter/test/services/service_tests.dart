// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/auth_service.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/signer_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-docs');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('SignerService', () {
    test('pubkey and lock state roundtrip through FFI', () async {
      final signer = SignerService();
      api.stubString('crateFfiSignerSignerPubkey', 'a1'.padRight(64, 'b'));
      api.stubBool('crateFfiSignerSignerIsLocked', false);
      api.stubBool('crateFfiSignerSignerLock', true);

      expect(await signer.pubkey(), 'a1'.padRight(64, 'b'));
      expect(await signer.isLocked(), isFalse);
      expect(await signer.lock(), isTrue);
    });

    test('keyring ops pass pubkey, never secret bytes', () async {
      final signer = SignerService();
      api.stubBool('crateFfiSignerSignerSaveToKeyring', true);
      api.stubBool('crateFfiSignerSignerUnlockFromKeyring', true);
      api.stubBool('crateFfiSignerSignerRemoveFromKeyring', true);
      api.stubString('crateFfiSignerSignerSignText', 'sig-hex');

      final pk = 'aa'.padRight(64, 'aa');
      expect(await signer.saveToKeyring(pk), isTrue);
      expect(await signer.unlockFromKeyring(pk), isTrue);
      expect(await signer.removeFromKeyring(pk), isTrue);

      for (final method in [
        'crateFfiSignerSignerSaveToKeyring',
        'crateFfiSignerSignerUnlockFromKeyring',
        'crateFfiSignerSignerRemoveFromKeyring',
      ]) {
        final inv = api.callsOf(method).single;
        expect(api.namedArg(inv, 'pubkey'), pk);
      }

      expect(await signer.signText('hello'), 'sig-hex');
      final signInv = api.callsOf('crateFfiSignerSignerSignText').single;
      expect(api.namedArg(signInv, 'message'), 'hello');
    });
  });

  group('AuthService', () {
    test('generateKeypair decodes and stores KeyPair', () async {
      final auth = AuthService();
      var notified = 0;
      auth.addListener(() => notified++);
      api.stubString(
        'crateFfiAuthAuthGenerateKeypair',
        '{"publicKey":"pk-hex","secretKey":"sk-hex"}',
      );

      final kp = await auth.generateKeypair();
      expect(kp.publicKey, 'pk-hex');
      expect(kp.toJson().containsKey('secret_key'), false); // secret never crosses FFI
      expect(auth.currentKeypair?.publicKey, 'pk-hex');
      expect(notified, 1);
    });

    test('mnemonic generate/validate/restore', () async {
      final auth = AuthService();
      const phrase = 'abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon about';
      api.stubString('crateFfiAuthAuthGenerateMnemonic', phrase);
      api.stubBool('crateFfiAuthAuthValidateMnemonic', true);
      api.stubStringBuilder('crateFfiAuthAuthRestoreFromMnemonic',
          (inv) => '{"publicKey":"pk","secretKey":"sk"}');

      expect(await auth.generateMnemonic(), phrase);
      expect(await auth.validateMnemonic(phrase), isTrue);
      final kp = await auth.restoreFromMnemonic(phrase, '');
      expect(kp.publicKey, 'pk');
      final inv = api.callsOf('crateFfiAuthAuthRestoreFromMnemonic').single;
      expect(api.namedArg(inv, 'mnemonic'), phrase);
    });

    test('errors surface in lastError and rethrow', () async {
      final auth = AuthService();
      api.stub('crateFfiAuthAuthGenerateKeypair',
          (_) => throw Exception('ffi boom'));
      await expectLater(auth.generateKeypair(), throwsException);
      expect(auth.lastError, contains('ffi boom'));
    });
  });

  group('SessionService', () {
    test('loadSession parses accounts and notifies', () async {
      final session = SessionService();
      var notified = 0;
      session.addListener(() => notified++);
      api.stubString(
        'crateFfiSessionSessionLoad',
        '{"active_pubkey":"pk-a","accounts":['
        '{"pubkey":"pk-a","npub":"npub1a","last_used":7,'
        '"relay_list":["wss://relay.example"]}]}',
      );

      final data = await session.loadSession();
      expect(data.activePubkey, 'pk-a');
      expect(session.activePubkey, 'pk-a');
      expect(session.getAccounts().length, 1);
      expect(session.getAccounts().first.relayList.single,
          'wss://relay.example');
      expect(session.hasActiveSession(), isTrue);
      expect(notified, 1);
    });

    test('addAccount makes first account active and persists', () async {
      final session = SessionService();
      api.stubString(
        'crateFfiSessionSessionLoad',
        '{"active_pubkey":null,"accounts":[]}',
      );
      api.stubBool('crateFfiSessionSessionAddAccount', true);

      await session.addAccount('pk-1', 'npub1', const ['wss://r']);
      expect(session.activePubkey, 'pk-1');
      await session.addAccount('pk-2', 'npub2', const []);
      expect(session.activePubkey, 'pk-1', reason: 'first stays active');
      expect(session.getAccounts().length, 2);
      expect(api.callCount('crateFfiSessionSessionAddAccount'), 2);
    });

    test('switchAccount requires existing account', () async {
      final session = SessionService();
      api.stubString(
        'crateFfiSessionSessionLoad',
        '{"active_pubkey":"pk-1","accounts":['
        '{"pubkey":"pk-1","npub":"npub1","last_used":1,"relay_list":[]}]}',
      );
      api.stubBool('crateFfiSessionSessionSwitchAccount', true);
      await session.loadSession();

      await session.switchAccount('pk-1');
      expect(session.activePubkey, 'pk-1');
      await expectLater(session.switchAccount('missing'), throwsException);
      expect(session.lastError, contains('Account not found'));
    });

    test('removeAccount clears active when last account', () async {
      final session = SessionService();
      api.stubString(
        'crateFfiSessionSessionLoad',
        '{"active_pubkey":"pk-1","accounts":['
        '{"pubkey":"pk-1","npub":"npub1","last_used":1,"relay_list":[]}]}',
      );
      await session.loadSession();
      await session.removeAccount('pk-1');
      expect(session.hasActiveSession(), isFalse);
      expect(session.getAccounts(), isEmpty);
      expect(session.activePubkey, isNull);
      expect(session.activeAccount, isNull);
    });

    test('saveSession encodes current state to FFI', () async {
      final session = SessionService();
      api.stubString(
        'crateFfiSessionSessionLoad',
        '{"active_pubkey":null,"accounts":[]}',
      );
      api.stubBool('crateFfiSessionSessionSave', true);
      await session.loadSession();
      expect(await session.saveSession(), isTrue);
      final inv = api.callsOf('crateFfiSessionSessionSave').single;
      final sent = api.namedArg(inv, 'sessionData') as String;
      expect(sent, contains('active_pubkey'));
    });
  });

  group('FeedService', () {
    String postsJson(int n) {
      final posts = List.generate(n, (i) => {
            'event_id': 'ev-$i',
            'pubkey': 'pk-$i',
            'content': 'post $i',
            'created_at': 1700000000 + i,
            'reactions': i,
            'replies': 0,
            'reposts': 0,
            'liked': false,
          });
      return jsonEncode(posts);
    }

    String postsJsonFrom(int n, int base) {
      final posts = List.generate(n, (i) => {
            'event_id': 'ev-${base + i}',
            'pubkey': 'pk-${base + i}',
            'content': 'post ${base + i}',
            'created_at': 1700000000 + base + i,
            'reactions': base + i,
            'replies': 0,
            'reposts': 0,
            'liked': false,
          });
      return jsonEncode(posts);
    }

    test('fetchFeed replaces on offset 0 and cap at 100 on paging',
        () async {
      final feed = FeedService();
      api.stub('crateFfiFeedFeedFetchEvents', (inv) {
        final options =
            jsonDecode(api.namedArg(inv, 'optionsJson') as String)
                as Map<String, dynamic>;
        return options['offset'] == 0
            ? postsJson(60)
            : postsJsonFrom(60, 100);
      });

      await feed.fetchFeed(limit: 60);
      expect(feed.posts.length, 60);
      expect(feed.isLoading, isFalse);

      await feed.loadMore(limit: 60);
      expect(feed.posts.length, 100, reason: 'capped at 100');
      // 120 unique posts dedup/sorted, cap keeps the newest 100 → front trimmed.
      expect(feed.posts.first.eventId, 'ev-20');
      expect(feed.posts.last.content, 'post 159');
    });

    test('fetchFeed error sets lastError and rethrows', () async {
      final feed = FeedService();
      api.stub('crateFfiFeedFeedFetchEvents',
          (_) => throw Exception('feed down'));
      await expectLater(feed.fetchFeed(), throwsException);
      expect(feed.lastError, contains('feed down'));
      expect(feed.isLoading, isFalse);
    });

    test('pinned posts load from settings and toggle persists', () async {
      final feed = FeedService();
      api.stub('crateFfiDbDbGetSetting', (_) => '["ev-1","ev-2"]');
      api.stubBool('crateFfiDbDbSetSetting', true);

      await feed.loadPinnedPosts();
      expect(feed.pinnedPosts, ['ev-1', 'ev-2']);
      expect(feed.isPinned('ev-1'), isTrue);
      expect(feed.isPinned('ev-9'), isFalse);

      await feed.togglePin('ev-1');
      expect(feed.isPinned('ev-1'), isFalse);
      expect(feed.isPinned('ev-2'), isTrue);
      final inv = api.callsOf('crateFfiDbDbSetSetting').single;
      expect(api.namedArg(inv, 'key'), 'pinned_posts');
      expect(api.namedArg(inv, 'value'), contains('ev-2'));
    });

    test('live insert dedups and live reaction bumps counters', () async {
      final feed = FeedService();
      api.stubString('crateFfiFeedFeedFetchEvents', '[]');
      api.stubBool('crateFfiFeedFeedValidateNote', true);
      await feed.fetchFeed();

      final post = FeedPost(
        eventId: 'ev-1',
        pubkey: 'pk-1',
        content: 'live',
        createdAt: 1,
        reactions: 0,
        replies: 0,
        reposts: 0,
        liked: false,
      );
      feed.insertLivePost(post);
      feed.insertLivePost(post);
      expect(feed.posts.length, 1);

      feed.applyLiveReaction('ev-1', 'pk-2', '+', 'r1');
      expect(feed.posts.first.reactions, 1);
      expect(feed.posts.first.liked, isTrue);
      // Same reaction event id is deduped; a distinct id bumps again.
      feed.applyLiveReaction('ev-1', 'pk-2', '+', 'r1');
      expect(feed.posts.first.reactions, 1);
      feed.applyLiveReaction('ev-1', 'pk-2', '+', 'r2');
      expect(feed.posts.first.reactions, 2);
      // Unlike decrements and never goes negative.
      feed.applyLiveReaction('ev-1', 'pk-2', '-', 'r3');
      expect(feed.posts.first.reactions, 1);
    });
  });

  group('MessagingService', () {
    test('insertLiveDm dedups and caps at 200', () {
      final ms = MessagingService();
      DirectMessage msg(String id) => DirectMessage(
            id: id,
            sender: 'peer-1',
            recipient: 'me',
            content: 'x',
            createdAt: 1,
            decrypted: true,
            isOwn: false,
          );
      ms.insertLiveDm(msg('a'));
      ms.insertLiveDm(msg('a'));
      expect(ms.conversations['peer-1']!.length, 1);

      for (var i = 0; i < 250; i++) {
        ms.insertLiveDm(msg('m-$i'));
      }
      expect(ms.conversations['peer-1']!.length, 200);
      expect(ms.conversations['peer-1']!.first.id, 'm-50');
    });

    test('resolvePubkey accepts hex and npub, rejects garbage', () async {
      final ms = MessagingService();
      api.stub('crateFfiAuthAuthNpubDecode', (inv) {
        final npub = api.namedArg(inv, 'npub') as String;
        if (npub == 'npub1bad') {
          throw Exception('invalid bech32');
        }
        return 'ab' * 32;
      });

      final hex = 'ab' * 32;
      expect(ms.resolvePubkey(hex), hex);
      expect(ms.resolvePubkey('npub1test'), 'ab' * 32);
      expect(() => ms.resolvePubkey('not-a-key'), throwsException);
      expect(() => ms.resolvePubkey('npub1bad'), throwsException);
    });

    test('sendDM appends own message to conversation', () async {
      final ms = MessagingService();
      api.stubString('crateFfiMessagingMessagingSendDm', 'ev-1');
      final id = await ms.sendDM('hi', 'peer-1', 'me');
      expect(id, 'ev-1');
      final conv = ms.conversations['peer-1']!;
      expect(conv.single.content, 'hi');
      expect(conv.single.isOwn, isTrue);
      final inv =
          api.callsOf('crateFfiMessagingMessagingSendDm').single;
      expect(api.namedArg(inv, 'recipientPubkey'), 'peer-1');
    });

    test('fetchDMs caches by peer and decodes payloads', () async {
      final ms = MessagingService();
      api.stubString(
        'crateFfiMessagingMessagingFetchDms',
        '[{"id":"m1","sender":"peer-1","recipient":"me",'
        '"content":"hello","created_at":5,"decrypted":true,"is_own":false}]',
      );
      final dms = await ms.fetchDMs('peer-1');
      expect(dms.single.content, 'hello');
      expect(dms.single.sender, 'peer-1');
      // Second call hits the in-memory cache, not FFI.
      await ms.fetchDMs('peer-1');
      expect(api.callCount('crateFfiMessagingMessagingFetchDms'), 1);
    });
  });
}