// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/dating_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

String cardJson(String pubkey, String name, {double score = 0.5}) {
  return '{"pubkey":"$pubkey","name":"$name","age":28,'
      '"location":"Berlin","bio":"hi","images":["img-1"],'
      '"interests":["music"],"compatibility_score":$score,'
      '"last_seen":1700000000}';
}

void main() {
  final env = bootstrapTestEnv('test-dating');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('DatingService', () {
    test('fetchProfiles parses cards, stores state and notifies', () async {
      final dating = DatingService();
      var notified = 0;
      dating.addListener(() => notified++);
      api.stubString(
        'crateFfiDatingDatingFetchProfiles',
        '[${cardJson('pk-a', 'Alice', score: 0.9)},'
        '${cardJson('pk-b', 'Bob', score: 0.7)}]',
      );

      final cards = await dating.fetchProfiles('me', limit: 25);
      expect(cards.length, 2);
      expect(dating.cards.length, 2);
      expect(dating.cards.first.name, 'Alice');
      expect(dating.cards.first.compatibilityScore, 0.9);
      expect(dating.cards.last.pubkey, 'pk-b');
      expect(notified, 1);

      final inv =
          api.callsOf('crateFfiDatingDatingFetchProfiles').single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
      expect(api.namedArg(inv, 'limit'), 25);
    });

    test('filterProfiles passes filters and parses result', () async {
      final dating = DatingService();
      api.stubString(
        'crateFfiDatingDatingFilterProfiles',
        '[${cardJson('pk-c', 'Cara', score: 0.8)}]',
      );

      final cards = await dating.filterProfiles(
        'me',
        minAge: 25,
        maxAge: 35,
        radiusKm: 10,
        interests: const ['music', 'hiking'],
      );
      expect(cards.single.name, 'Cara');
      expect(dating.cards.single.pubkey, 'pk-c');

      final inv =
          api.callsOf('crateFfiDatingDatingFilterProfiles').single;
      expect(api.namedArg(inv, 'minAge'), 25);
      expect(api.namedArg(inv, 'maxAge'), 35);
      expect(api.namedArg(inv, 'locationRadiusKm'), 10);
      expect(api.namedArg(inv, 'interestsJson'),
          jsonEncode(['music', 'hiking']));
    });

    test('calculateScore returns double and passes targetPubkey', () async {
      final dating = DatingService();
      api.stub('crateFfiDatingDatingCalculateScore', (_) => 87.5);

      expect(await dating.calculateScore('me', 'pk-a'), 87.5);
      final inv =
          api.callsOf('crateFfiDatingDatingCalculateScore').single;
      expect(api.namedArg(inv, 'targetPubkey'), 'pk-a');
      expect(api.namedArg(inv, 'preferencesJson'), '{}');
      expect(dating.lastError, isNull);
    });

    test('like and pass remove card from deck and call FFI', () async {
      final dating = DatingService();
      api.stubString(
        'crateFfiDatingDatingFetchProfiles',
        '[${cardJson('pk-a', 'Alice')},${cardJson('pk-b', 'Bob')}]',
      );
      await dating.fetchProfiles('me');
      expect(dating.cards.length, 2);

      api.stubBool('crateFfiDatingDatingLike', true);
      api.stubBool('crateFfiDatingDatingPass', true);

      expect(await dating.like('me', 'pk-a'), isTrue);
      expect(dating.cards.single.pubkey, 'pk-b');
      var inv = api.callsOf('crateFfiDatingDatingLike').single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
      expect(api.namedArg(inv, 'profileId'), 'pk-a');

      expect(await dating.pass('me', 'pk-b'), isTrue);
      expect(dating.cards, isEmpty);
      inv = api.callsOf('crateFfiDatingDatingPass').single;
      expect(api.namedArg(inv, 'profileId'), 'pk-b');
    });

    test('fetchMatches and fetchLikes fill state and notify', () async {
      final dating = DatingService();
      var notified = 0;
      dating.addListener(() => notified++);
      api.stubString(
        'crateFfiDatingDatingFetchMatches',
        '[${cardJson('pk-m', 'Mia')}]',
      );
      api.stubString(
        'crateFfiDatingDatingFetchLikes',
        '[${cardJson('pk-l', 'Leo')}]',
      );

      final matches = await dating.fetchMatches('me');
      expect(matches.single.name, 'Mia');
      expect(dating.matches.single.pubkey, 'pk-m');

      final likes = await dating.fetchLikes('me');
      expect(likes.single.name, 'Leo');
      expect(dating.likes.single.pubkey, 'pk-l');
      expect(notified, 2);
    });

    test('createProfile persists then reloads own profile', () async {
      final dating = DatingService();
      api.stubString('crateFfiDatingDatingCreateProfile', 'ev-1');
      api.stubString(
        'crateFfiDatingDatingGetOwnProfile',
        cardJson('me', 'Me'),
      );

      final id = await dating.createProfile(
          'me', 'Me', 30, 'Berlin', 'bio', const ['img'], const ['art']);
      expect(id, 'ev-1');
      expect(dating.ownProfile?.name, 'Me');

      final inv =
          api.callsOf('crateFfiDatingDatingCreateProfile').single;
      expect(api.namedArg(inv, 'name'), 'Me');
      expect(api.namedArg(inv, 'age'), 30);
      expect(api.namedArg(inv, 'imagesJson'), jsonEncode(['img']));
      expect(api.namedArg(inv, 'interestsJson'), jsonEncode(['art']));
      expect(api.callCount('crateFfiDatingDatingGetOwnProfile'), 1);
    });

    test('deleteProfile clears own profile state', () async {
      final dating = DatingService();
      api.stubString(
        'crateFfiDatingDatingGetOwnProfile',
        cardJson('me', 'Me'),
      );
      await dating.getOwnProfile('me');
      expect(dating.ownProfile, isNotNull);

      api.stubBool('crateFfiDatingDatingDeleteProfile', true);
      expect(await dating.deleteProfile('me'), isTrue);
      expect(dating.ownProfile, isNull);
      final inv =
          api.callsOf('crateFfiDatingDatingDeleteProfile').single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
    });

    test('getStats parses counts and completeness', () async {
      final dating = DatingService();
      api.stubString(
        'crateFfiDatingDatingGetStats',
        '{"profile_views":12,"likes_received":5,"superlike_received":1,'
        '"matches":3,"profile_complete":true,"photo_count":4}',
      );

      final stats = await dating.getStats('me');
      expect(stats.profileViews, 12);
      expect(stats.likesReceived, 5);
      expect(stats.superlikeReceived, 1);
      expect(stats.matches, 3);
      expect(stats.profileComplete, isTrue);
      expect(stats.photoCount, 4);
      final inv = api.callsOf('crateFfiDatingDatingGetStats').single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
    });

    test('error sets lastError and rethrows', () async {
      final dating = DatingService();
      api.stub('crateFfiDatingDatingFetchProfiles',
          (_) => throw Exception('dating down'));

      await expectLater(dating.fetchProfiles('me'), throwsException);
      expect(dating.lastError, contains('dating down'));
    });

    test('unmatch swallows errors and returns false', () async {
      final dating = DatingService();
      api.stub('crateFfiDatingDatingUnmatch',
          (_) => throw Exception('no store'));

      expect(await dating.unmatch('me', 'pk-a'), isFalse);
      expect(dating.lastError, contains('no store'));
    });
  });
}
