// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/friends_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

ProfileInfo profile(String pubkey, String name) => ProfileInfo(
      pubkey: pubkey,
      name: name,
      displayName: name,
      picture: '',
      banner: '',
      about: '',
      nip05: '',
      nip05Valid: false,
      createdAt: 0,
      followers: 0,
      following: 0,
      isFollowing: false,
      wotStatus: 'unknown',
    );

void main() {
  final env = bootstrapTestEnv('test-friends');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('FriendsService', () {
    test('fetchSuggestions loads pubkey list, flags loaded and notifies',
        () async {
      final friends = FriendsService();
      var notified = 0;
      friends.addListener(() => notified++);
      api.stubListString(
          'crateFfiSocialSocialFriendSuggestions', const ['pk-1', 'pk-2']);

      final suggestions = await friends.fetchSuggestions();
      expect(suggestions, ['pk-1', 'pk-2']);
      expect(friends.suggestions, ['pk-1', 'pk-2']);
      expect(friends.suggestionsLoaded, isTrue);
      expect(friends.lastError, isNull);
      expect(notified, 1);
    });

    test('fetchSuggestions with empty list still marks loaded', () async {
      final friends = FriendsService();
      api.stubListString('crateFfiSocialSocialFriendSuggestions', const []);

      final suggestions = await friends.fetchSuggestions();
      expect(suggestions, isEmpty);
      expect(friends.suggestionsLoaded, isTrue);
    });

    test('fetchSuggestions failure sets lastError and rethrows', () async {
      final friends = FriendsService();
      api.stub('crateFfiSocialSocialFriendSuggestions',
          (_) => throw Exception('graph down'));

      await expectLater(friends.fetchSuggestions(), throwsException);
      expect(friends.lastError, contains('graph down'));
      expect(friends.suggestionsLoaded, isFalse);
    });

    test('sendFriendRequest returns accept flag and passes pubkey', () async {
      final friends = FriendsService();
      api.stubBool('crateFfiRelationsRelationsSendFriendRequest', true);

      final ok = await friends.sendFriendRequest('pk-9');
      expect(ok, isTrue);
      expect(friends.lastError, isNull);

      final inv =
          api.callsOf('crateFfiRelationsRelationsSendFriendRequest').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-9');
    });

    test('sendFriendRequest rejection surfaces as false', () async {
      final friends = FriendsService();
      api.stubBool('crateFfiRelationsRelationsSendFriendRequest', false);

      expect(await friends.sendFriendRequest('pk-9'), isFalse);
    });

    test('sendFriendRequest failure sets lastError and rethrows', () async {
      final friends = FriendsService();
      api.stub('crateFfiRelationsRelationsSendFriendRequest',
          (_) => throw Exception('relay refused'));

      await expectLater(friends.sendFriendRequest('pk-9'), throwsException);
      expect(friends.lastError, contains('relay refused'));
    });

    test('addContact appends, dedupes and notifies', () async {
      final friends = FriendsService();
      var notified = 0;
      friends.addListener(() => notified++);

      friends.addContact(profile('pk-1', 'alice'));
      friends.addContact(profile('pk-2', 'bob'));
      friends.addContact(profile('pk-1', 'alice'));

      expect(friends.contacts.length, 2);
      expect(friends.contacts.map((c) => c.pubkey), ['pk-1', 'pk-2']);
      expect(notified, 2);
    });

    test('removeContact deletes matching pubkey only', () async {
      final friends = FriendsService();
      friends.addContact(profile('pk-1', 'alice'));
      friends.addContact(profile('pk-2', 'bob'));

      friends.removeContact('pk-1');
      expect(friends.contacts.single.pubkey, 'pk-2');

      friends.removeContact('pk-9');
      expect(friends.contacts.single.pubkey, 'pk-2');
    });

    test('clearError resets lastError and notifies', () async {
      final friends = FriendsService();
      api.stub('crateFfiSocialSocialFriendSuggestions',
          (_) => throw Exception('boom'));
      await expectLater(friends.fetchSuggestions(), throwsException);
      expect(friends.lastError, isNotNull);

      var notified = 0;
      friends.addListener(() => notified++);
      friends.clearError();
      expect(friends.lastError, isNull);
      expect(notified, 1);
    });
  });
}
