// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/bookmarks_service.dart';
import 'package:soshal_flutter/services/calls_service.dart';
import 'package:soshal_flutter/services/dating_service.dart';
import 'package:soshal_flutter/services/events_service.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/friends_service.dart';
import 'package:soshal_flutter/services/groups_service.dart';
import 'package:soshal_flutter/services/marketplace_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/minis_service.dart';
import 'package:soshal_flutter/services/moderation_service.dart';
import 'package:soshal_flutter/services/music_service.dart';
import 'package:soshal_flutter/services/notifications_service.dart';
import 'package:soshal_flutter/services/search_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/turso_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-session');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('SessionService', () {
    test('loadSession parses and stores session data', () async {
      final session = SessionService();
      var notified = 0;
      session.addListener(() => notified++);

      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": ["relay1"]}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      final data = await session.loadSession();
      expect(data.activePubkey, 'pk1');
      expect(data.accounts.length, 1);
      expect(data.accounts.first.pubkey, 'pk1');
      expect(data.accounts.first.npub, 'npub1');
      expect(session.activePubkey, 'pk1');
      expect(session.session, isNotNull);
      expect(notified, 1);
      expect(session.lastError, isNull);
    });

    test('loadSession clears previous error', () async {
      final session = SessionService();
      session.setLastError(Exception('old'), StackTrace.current);

      const sessionJson = '{"active_pubkey": null, "accounts": []}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      expect(session.lastError, isNull);
    });

    test('loadSession sets error on exception', () async {
      final session = SessionService();
      api.stub('crateFfiSessionSessionLoad', (_) {
        throw Exception('Load failed');
      });
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await expectLater(session.loadSession(), throwsException);
      expect(session.lastError, isNotNull);
    });

    test('saveSession requires session to be loaded', () async {
      final session = SessionService();
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      expect(() => session.saveSession(), throwsException);
      expect(session.lastError, isNotNull);
    });

    test('saveSession saves current session state', () async {
      final session = SessionService();
      const sessionJson = '{"active_pubkey": "pk1", "accounts": []}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
      api.stubBool('crateFfiSessionSessionSave', true);

      await session.loadSession();
      final result = await session.saveSession();

      expect(result, true);
      final inv = api.callsOf('crateFfiSessionSessionSave').single;
      expect(api.namedArg(inv, 'dbPath'), '${env.$2}/soshal.db');
      expect(api.namedArg(inv, 'sessionData'), isNotNull);
    });

    test('addAccount auto-loads session if needed', () async {
      final session = SessionService();
      const sessionJson = '{"active_pubkey": null, "accounts": []}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
      api.stub("crateFfiSessionSessionAddAccount", (_) => true);

      expect(session.session, isNull);
      await session.addAccount('newpk', 'newpub', ['relay1']);
      expect(session.session, isNotNull);
    });

    test('addAccount adds account and sets as active if first', () async {
      final session = SessionService();
      const sessionJson = '{"active_pubkey": null, "accounts": []}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
      api.stub("crateFfiSessionSessionAddAccount", (_) => true);

      var notified = 0;
      session.addListener(() => notified++);

      await session.loadSession();
      expect(session.activePubkey, isNull);

      await session.addAccount('pk1', 'npub1', ['relay1', 'relay2']);

      expect(session.session!.accounts.length, 1);
      expect(session.session!.accounts.first.pubkey, 'pk1');
      expect(session.session!.accounts.first.relayList, ['relay1', 'relay2']);
      expect(session.activePubkey, 'pk1');
      expect(notified, 2);
    });

    test('addAccount adds but does not activate if account already active', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [{"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []}]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
      api.stub("crateFfiSessionSessionAddAccount", (_) => true);

      await session.loadSession();
      expect(session.activePubkey, 'pk1');

      await session.addAccount('pk2', 'npub2', ['relay1']);

      expect(session.session!.accounts.length, 2);
      expect(session.activePubkey, 'pk1');
    });

    test('storeProfile calls bridge', () async {
      const profileJson = '{"pubkey":"pk1","name":"Alice"}';
      api.stubBool('crateFfiIdentityIdentityStoreProfile', true);

      final session = SessionService();
      final result = await session.storeProfile(profileJson);

      expect(result, true);
      final inv = api.callsOf('crateFfiIdentityIdentityStoreProfile').single;
      expect(api.namedArg(inv, 'profile'), profileJson);
    });

    test('updateRelays modifies relay list and saves', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [{"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": ["old"]}]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
      api.stubBool('crateFfiSessionSessionSave', true);

      await session.loadSession();
      expect(session.session!.accounts.first.relayList, ['old']);

      var notified = 0;
      session.addListener(() => notified++);

      await session.updateRelays('pk1', ['new1', 'new2']);

      expect(session.session!.accounts.first.relayList, ['new1', 'new2']);
      expect(notified, 1);
      expect(api.callsOf('crateFfiSessionSessionSave').length, 1);
    });

    test('updateRelays does nothing if session not loaded', () async {
      final session = SessionService();
      api.stubBool('crateFfiSessionSessionSave', true);

      var notified = 0;
      session.addListener(() => notified++);

      await session.updateRelays('pk1', ['relay1']);
      expect(notified, 0);
    });

    test('updateRelays does nothing if account not found', () async {
      final session = SessionService();
      const sessionJson =
          '{"active_pubkey": "pk1", "accounts": [{"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []}]}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      var notified = 0;
      session.addListener(() => notified++);

      await session.updateRelays('nonexistent', ['relay1']);
      expect(notified, 0);
    });

    test('switchAccount changes active account', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 2000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
      api.stub('crateFfiSessionSessionSwitchAccount', (_) => true);

      await session.loadSession();
      expect(session.activePubkey, 'pk1');

      var notified = 0;
      session.addListener(() => notified++);

      await session.switchAccount('pk2');

      expect(session.activePubkey, 'pk2');
      expect(notified, 1);
      final inv = api.callsOf('crateFfiSessionSessionSwitchAccount').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk2');
    });

    test('switchAccount fails if account does not exist', () async {
      final session = SessionService();
      const sessionJson = '{"active_pubkey": "pk1", "accounts": []}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();

      expect(
        () => session.switchAccount('nonexistent'),
        throwsException,
      );
      await Future<void>.value();
      expect(session.lastError, isNotNull);
    });

    test('removeAccount removes from list', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 2000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      expect(session.session!.accounts.length, 2);

      var notified = 0;
      session.addListener(() => notified++);

      await session.removeAccount('pk2');

      expect(session.session!.accounts.length, 1);
      expect(session.session!.accounts.first.pubkey, 'pk1');
      expect(notified, 1);
    });

    test('removeAccount clears active if removed account is active', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [{"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []}]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      expect(session.activePubkey, 'pk1');

      await session.removeAccount('pk1');

      expect(session.activePubkey, isNull);
      expect(session.session!.accounts.isEmpty, true);
    });

    test('removeAccount switches to first if multiple exist', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 2000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      await session.removeAccount('pk1');

      expect(session.activePubkey, 'pk2');
    });

    test('hasActiveSession returns correct status', () async {
      final session = SessionService();
      expect(session.hasActiveSession(), false);

      const sessionJson = '{"active_pubkey": "pk1", "accounts": []}';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      expect(session.hasActiveSession(), true);
    });

    test('getAccounts returns account list', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 2000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();

      final accounts = session.getAccounts();
      expect(accounts.length, 2);
      expect(accounts[0].pubkey, 'pk1');
      expect(accounts[1].pubkey, 'pk2');
    });

    test('getAccounts returns empty list before load', () async {
      final session = SessionService();
      final accounts = session.getAccounts();
      expect(accounts.isEmpty, true);
    });

    test('activeAccount returns correct account', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": ["relay1"]},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 2000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();

      final active = session.activeAccount;
      expect(active!.pubkey, 'pk1');
      expect(active.npub, 'npub1');
    });

    test('lastUsedAccount returns account with highest lastUsed timestamp', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": "pk1",
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 5000, "relay_list": []},
          {"pubkey": "pk3", "npub": "npub3", "last_used": 3000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      await session.loadSession();
      expect(session.lastUsedAccount?.pubkey, 'pk2');
    });

    test('loadSession defaults activePubkey to lastUsedAccount when active_pubkey is null', () async {
      final session = SessionService();
      const sessionJson = '''{
        "active_pubkey": null,
        "accounts": [
          {"pubkey": "pk1", "npub": "npub1", "last_used": 1000, "relay_list": []},
          {"pubkey": "pk2", "npub": "npub2", "last_used": 5000, "relay_list": []}
        ]
      }''';
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);

      final data = await session.loadSession();
      expect(data.activePubkey, 'pk2');
      expect(session.activePubkey, 'pk2');
      expect(session.activeAccount?.pubkey, 'pk2');
    });

    test('SessionAccount.fromJson parses all fields', () {
      const json = {
        'pubkey': 'pk1',
        'npub': 'npub1',
        'last_used': 1234567890,
        'relay_list': ['relay1', 'relay2']
      };
      final acc = SessionAccount.fromJson(json);
      expect(acc.pubkey, 'pk1');
      expect(acc.npub, 'npub1');
      expect(acc.lastUsed, 1234567890);
      expect(acc.relayList, ['relay1', 'relay2']);
    });

    test('SessionAccount.toJson emits snake_case', () {
      final acc =
          SessionAccount(pubkey: 'pk', npub: 'n', lastUsed: 1000, relayList: []);
      final json = acc.toJson();
      expect(json['pubkey'], 'pk');
      expect(json['npub'], 'n');
      expect(json['last_used'], 1000);
      expect(json['relay_list'], []);
    });

    test('SessionData.fromJson parses nested accounts', () {
      const json = {
        'active_pubkey': 'pk1',
        'accounts': [
          {'pubkey': 'pk1', 'npub': 'npub1', 'last_used': 1000, 'relay_list': []},
          {'pubkey': 'pk2', 'npub': 'npub2', 'last_used': 2000, 'relay_list': []}
        ]
      };
      final data = SessionData.fromJson(json);
      expect(data.activePubkey, 'pk1');
      expect(data.accounts.length, 2);
      expect(data.accounts[0].pubkey, 'pk1');
      expect(data.accounts[1].pubkey, 'pk2');
    });

    test('reset clears friends service audience on account switch', () {
      final session = SessionService();
      final friends = FriendsService();
      friends.friendPubkeys.add('friend_pk');
      expect(friends.friendPubkeys, contains('friend_pk'));

      session.attachAccountScopedServices(
        feed: FeedService(),
        messaging: MessagingService(),
        notifications: NotificationService(),
        search: SearchService(),
        dating: DatingService(),
        marketplace: MarketplaceService(),
        events: EventsService(),
        groups: GroupsService(),
        bookmarks: BookmarksService(),
        moderation: ModerationService(),
        calls: CallsService(),
        friends: friends,
      );

      session.reset();
      expect(friends.friendPubkeys, isEmpty);
    });

    test('reset clears minis and music services on account switch', () {
      final session = SessionService();
      final minis = MinisService();
      final music = MusicService();
      minis.savedMinis.add(MiniItem(
        id: 'mini1',
        pubkey: 'pk',
        videoUrl: 'https://example.com/video.mp4',
        blobHash: '',
        mediaSize: 100,
        textOverlay: '',
        thumbnail: '',
        audience: 'public',
        createdAt: 1000,
        reactions: 0,
        liked: false,
      ));
      music.savedTracks.add(MusicTrack(
        id: 'track1',
        pubkey: 'pk',
        audioUrl: 'https://example.com/audio.mp3',
        blobHash: '',
        mediaSize: 100,
        title: 'Track',
        thumbnail: '',
        hashtags: const [],
        d: '',
        audience: 'public',
        createdAt: 1000,
      ));
      music.playlists.add(MusicPlaylist(
        id: 'pl1',
        title: 'Playlist',
        isPrivate: false,
        createdAt: 1000,
        trackCount: 1,
      ));
      expect(minis.savedMinis, isNotEmpty);
      expect(music.savedTracks, isNotEmpty);
      expect(music.playlists, isNotEmpty);

      session.attachAccountScopedServices(
        feed: FeedService(),
        messaging: MessagingService(),
        notifications: NotificationService(),
        search: SearchService(),
        dating: DatingService(),
        marketplace: MarketplaceService(),
        events: EventsService(),
        groups: GroupsService(),
        bookmarks: BookmarksService(),
        moderation: ModerationService(),
        calls: CallsService(),
        minis: minis,
        music: music,
      );

      session.reset();
      expect(minis.savedMinis, isEmpty);
      expect(music.savedTracks, isEmpty);
      expect(music.playlists, isEmpty);
    });

    test('reset clears turso service on account switch', () async {
      final session = SessionService();
      final turso = TursoService();
      api.stubString('crateFfiTursoDbTursoConfigure', 'Turso database credentials saved');
      api.stubString('crateFfiTursoDbTursoStatus', '{"configured":true,"status":"idle"}');
      await turso.configure(url: 'https://example.turso.io', authToken: 'tok');
      expect(turso.isConfigured, isTrue);
      expect(turso.url, 'https://example.turso.io');

      session.attachAccountScopedServices(
        feed: FeedService(),
        messaging: MessagingService(),
        notifications: NotificationService(),
        search: SearchService(),
        dating: DatingService(),
        marketplace: MarketplaceService(),
        events: EventsService(),
        groups: GroupsService(),
        bookmarks: BookmarksService(),
        moderation: ModerationService(),
        calls: CallsService(),
        turso: turso,
      );

      session.reset();
      expect(turso.isConfigured, isFalse);
      expect(turso.url, isEmpty);
    });

    test('reset clears shell service state on account switch', () async {
      final session = SessionService();
      final shell = ShellService();
      final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      api.stubString('crateFfiCallsCallsFetchSignals', '[{"kind":20001,"created_at":$now,"call_id":"c1"}]');
      await shell.pollCallSignals('mypk');
      expect(shell.incomingCall, isNotNull);

      session.attachAccountScopedServices(
        feed: FeedService(),
        messaging: MessagingService(),
        notifications: NotificationService(),
        search: SearchService(),
        dating: DatingService(),
        marketplace: MarketplaceService(),
        events: EventsService(),
        groups: GroupsService(),
        bookmarks: BookmarksService(),
        moderation: ModerationService(),
        calls: CallsService(),
        shell: shell,
      );

      session.reset();
      expect(shell.incomingCall, isNull);
    });
  });
}
