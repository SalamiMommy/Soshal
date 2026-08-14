// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/legacy_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-legacy');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('LegacyService - Huddle Posts', () {
    test('huddlePostStore stores post', () async {
      api.stubBool('crateFfiLegacyLegacyHuddlePostStore', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.huddlePostStore(
        huddleId: 'huddle1',
        pubkey: 'pk1',
        content: 'hello',
        expiresInSecs: 3600,
      );

      expect(result, true);
      expect(notified, 1);
      final inv = api.callsOf('crateFfiLegacyLegacyHuddlePostStore').single;
      expect(api.namedArg(inv, 'huddleId'), 'huddle1');
      expect(api.namedArg(inv, 'expiresInSecs'), 3600);
    });

    test('huddlePostStore returns false on error', () async {
      api.stub('crateFfiLegacyLegacyHuddlePostStore', (_) {
        throw Exception('Store failed');
      });

      final legacy = LegacyService();
      final result =
          await legacy.huddlePostStore(huddleId: 'h', pubkey: 'p', content: 'c');

      expect(result, false);
      expect(legacy.lastError, isNotNull);
    });

    test('huddlePosts fetches posts', () async {
      const postsJson =
          '[{"id":"p1","content":"msg1"},{"id":"p2","content":"msg2"}]';
      api.stubString('crateFfiLegacyLegacyHuddlePosts', postsJson);

      final legacy = LegacyService();
      final result = await legacy.huddlePosts('huddle1', limit: 50);

      expect(result.length, 2);
      expect(result[0]['id'], 'p1');
    });

    test('huddlePosts returns empty on error', () async {
      api.stub('crateFfiLegacyLegacyHuddlePosts', (_) {
        throw Exception('Fetch failed');
      });

      final legacy = LegacyService();
      final result = await legacy.huddlePosts('h');

      expect(result.isEmpty, true);
      expect(legacy.lastError, isNotNull);
    });

    test('huddlePostDelete deletes post', () async {
      api.stubBool('crateFfiLegacyLegacyHuddlePostDelete', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.huddlePostDelete('postid');

      expect(result, true);
      expect(notified, 1);
    });
  });

  group('LegacyService - Guestbook', () {
    test('guestbookAdd adds entry', () async {
      api.stubBool('crateFfiLegacyLegacyGuestbookAdd', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.guestbookAdd(
        profilePubkey: 'profile_pk',
        senderPubkey: 'sender_pk',
        senderName: 'Alice',
        content: 'nice profile!',
      );

      expect(result, true);
      expect(notified, 1);
    });

    test('guestbookEntries fetches entries', () async {
      const entriesJson =
          '[{"id":"e1","content":"msg1","approved":true}]';
      api.stubString('crateFfiLegacyLegacyGuestbookEntries', entriesJson);

      final legacy = LegacyService();
      final result =
          await legacy.guestbookEntries('profile_pk', limit: 50, onlyApproved: true);

      expect(result.length, 1);
      expect(result[0]['content'], 'msg1');
      final inv = api.callsOf('crateFfiLegacyLegacyGuestbookEntries').single;
      expect(api.namedArg(inv, 'onlyApproved'), true);
    });

    test('guestbookSetApproved approves entry', () async {
      api.stubBool('crateFfiLegacyLegacyGuestbookSetApproved', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.guestbookSetApproved('entryid', true);

      expect(result, true);
      expect(notified, 1);
      final inv = api.callsOf('crateFfiLegacyLegacyGuestbookSetApproved').single;
      expect(api.namedArg(inv, 'approved'), true);
    });

    test('guestbookDelete deletes entry', () async {
      api.stubBool('crateFfiLegacyLegacyGuestbookDelete', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.guestbookDelete('entryid');

      expect(result, true);
      expect(notified, 1);
    });
  });

  group('LegacyService - Stream Chat', () {
    test('streamChatSend sends message', () async {
      api.stubBool('crateFfiLegacyLegacyStreamChatSend', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.streamChatSend(
        streamId: 'stream1',
        pubkey: 'pk1',
        text: 'hello',
      );

      expect(result, true);
      expect(notified, 1);
    });

    test('streamChatMessages fetches messages', () async {
      const messagesJson =
          '[{"id":"m1","text":"hello"},{"id":"m2","text":"world"}]';
      api.stubString('crateFfiLegacyLegacyStreamChatMessages', messagesJson);

      final legacy = LegacyService();
      final result = await legacy.streamChatMessages('stream1', limit: 100);

      expect(result.length, 2);
      expect(result[0]['text'], 'hello');
    });

    test('streamChatClear clears messages', () async {
      api.stubBool('crateFfiLegacyLegacyStreamChatClear', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.streamChatClear('stream1');

      expect(result, true);
      expect(notified, 1);
    });
  });

  group('LegacyService - Link Previews', () {
    test('linkPreviewStore stores preview', () async {
      api.stubBool('crateFfiLegacyLegacyLinkPreviewStore', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.linkPreviewStore(
        url: 'https://example.com',
        title: 'Example Site',
        description: 'A test site',
      );

      expect(result, true);
      expect(notified, 1);
    });

    test('linkPreviewGet retrieves preview', () async {
      const previewJson = '{"title":"Example","description":"test"}';
      api.stubString('crateFfiLegacyLegacyLinkPreviewGet', previewJson);

      final legacy = LegacyService();
      final result = await legacy.linkPreviewGet('https://example.com');

      expect(result, isNotNull);
      expect(result!['title'], 'Example');
    });

    test('linkPreviewGet returns null when not found', () async {
      api.stubString('crateFfiLegacyLegacyLinkPreviewGet', '');

      final legacy = LegacyService();
      final result = await legacy.linkPreviewGet('https://notfound.com');

      expect(result, isNull);
    });
  });

  group('LegacyService - Friend Backups', () {
    test('friendBackupStore stores encrypted backup', () async {
      api.stubBool('crateFfiLegacyLegacyFriendBackupStore', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.friendBackupStore(
        'userpk',
        'encrypted_data_here',
      );

      expect(result, true);
      expect(notified, 1);
    });

    test('friendBackupGet retrieves backup', () async {
      api.stubString('crateFfiLegacyLegacyFriendBackupGet', 'encrypted_backup');

      final legacy = LegacyService();
      final result = await legacy.friendBackupGet('userpk');

      expect(result, 'encrypted_backup');
    });

    test('friendBackupDelete removes backup', () async {
      api.stubBool('crateFfiLegacyLegacyFriendBackupDelete', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.friendBackupDelete('userpk');

      expect(result, true);
      expect(notified, 1);
    });
  });

  group('LegacyService - Geohash Peers', () {
    test('geohashPeerUpsert records location peer', () async {
      api.stubBool('crateFfiLegacyLegacyGeohashPeerUpsert', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.geohashPeerUpsert(
        pubkey: 'pk1',
        geohash: 'u0deg3',
        purpose: 'both',
      );

      expect(result, true);
      expect(notified, 1);
    });

    test('geohashPeersByCell fetches peers', () async {
      const peersJson = '[{"pubkey":"pk1","geohash":"u0deg3"}]';
      api.stubString('crateFfiLegacyLegacyGeohashPeersByCell', peersJson);

      final legacy = LegacyService();
      final result = await legacy.geohashPeersByCell('u0deg3');

      expect(result.length, 1);
      expect(result[0]['pubkey'], 'pk1');
    });

    test('geohashPeersPurge removes stale peers', () async {
      api.stub('crateFfiLegacyLegacyGeohashPeersPurge', (_) {
        return BigInt.from(5);
      });

      final legacy = LegacyService();
      final result = await legacy.geohashPeersPurge(86400);

      expect(result, BigInt.from(5));
    });
  });

  group('LegacyService - Profile Nodes', () {
    test('profileNodeUpsert creates/updates node', () async {
      api.stubBool('crateFfiLegacyLegacyProfileNodeUpsert', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.profileNodeUpsert(
        id: 'node1',
        userPubkey: 'userpk',
        nodeType: 'text',
        styles: '{"color":"red"}',
        properties: '{"text":"hello"}',
        layout: '{"row":0,"col":0}',
      );

      expect(result, true);
      expect(notified, 1);
    });

    test('profileNodes fetches all nodes', () async {
      const nodesJson = '[{"id":"n1","type":"text"}]';
      api.stubString('crateFfiLegacyLegacyProfileNodes', nodesJson);

      final legacy = LegacyService();
      final result = await legacy.profileNodes('userpk');

      expect(result.length, 1);
      expect(result[0]['id'], 'n1');
    });

    test('profileNodeDelete removes node', () async {
      api.stub('crateFfiLegacyLegacyProfileNodeDelete', (_) {
        return BigInt.one;
      });

      final legacy = LegacyService();
      final result = await legacy.profileNodeDelete('nodeid', 'userpk');

      expect(result, BigInt.one);
    });
  });

  group('LegacyService - Refetch Markers', () {
    test('refetchBlock marks block for refetch', () async {
      api.stubBool('crateFfiLegacyLegacyRefetchBlock', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.refetchBlock('blockid', 'test error');

      expect(result, true);
      expect(notified, 1);
    });

    test('refetchBlocked checks if blocked', () async {
      api.stubBool('crateFfiLegacyLegacyRefetchBlocked', true);

      final legacy = LegacyService();
      final result = await legacy.refetchBlocked('blockid');

      expect(result, true);
    });

    test('refetchUnblock clears block', () async {
      api.stubBool('crateFfiLegacyLegacyRefetchUnblock', true);

      final legacy = LegacyService();
      var notified = 0;
      legacy.addListener(() => notified++);

      final result = await legacy.refetchUnblock('blockid');

      expect(result, true);
      expect(notified, 1);
    });
  });

  group('LegacyService - Diagnostics', () {
    test('diagnosticLog records log', () async {
      api.stubBool('crateFfiLegacyLegacyDiagnosticLog', true);

      final legacy = LegacyService();
      final result = await legacy.diagnosticLog(
        level: 'error',
        service: 'auth',
        method: 'login',
        message: 'Connection timeout',
      );

      expect(result, true);
      final inv = api.callsOf('crateFfiLegacyLegacyDiagnosticLog').single;
      expect(api.namedArg(inv, 'level'), 'error');
      expect(api.namedArg(inv, 'service'), 'auth');
    });

    test('diagnosticLogs fetches logs', () async {
      const logsJson =
          '[{"level":"error","service":"auth","message":"fail"}]';
      api.stubString('crateFfiLegacyLegacyDiagnosticLogs', logsJson);

      final legacy = LegacyService();
      final result = await legacy.diagnosticLogs(limit: 100, level: 'error');

      expect(result.length, 1);
      expect(result[0]['level'], 'error');
    });

    test('diagnosticPurge removes old logs', () async {
      api.stub('crateFfiLegacyLegacyDiagnosticPurge', (_) {
        return BigInt.from(42);
      });

      final legacy = LegacyService();
      final result = await legacy.diagnosticPurge(86400);

      expect(result, BigInt.from(42));
    });
  });
}
