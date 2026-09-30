// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/friends_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';

import '../helpers/test_env.dart';

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

    test('clearLastError resets lastError', () async {
      final friends = FriendsService();
      api.stub('crateFfiSocialSocialFriendSuggestions',
          (_) => throw Exception('boom'));
      await expectLater(friends.fetchSuggestions(), throwsException);
      expect(friends.lastError, isNotNull);

      friends.clearLastError();
      expect(friends.lastError, isNull);
    });

    test('fetchFollowsUnion returns the decoded deduped array', () async {
      final friends = FriendsService();
      api.stub('crateFfiIdentityIdentityFetchFollowsUnion', (inv) {
        final pks = api.namedArg(inv, 'pubkeys') as List<dynamic>;
        return jsonEncode(['u-${pks.length}']);
      });

      final out = await friends.fetchFollowsUnion(['a', 'b', 'c']);

      expect(out, ['u-3']);
      final inv = api.callsOf('crateFfiIdentityIdentityFetchFollowsUnion').single;
      expect(api.namedArg(inv, 'pubkeys'), ['a', 'b', 'c']);
    });

    test('fetchFollowsUnion tolerates a non-array payload and empty input',
        () async {
      final friends = FriendsService();
      api.stub('crateFfiIdentityIdentityFetchFollowsUnion', (_) => 'null');
      expect(await friends.fetchFollowsUnion(['a']), isEmpty);
      expect(await friends.fetchFollowsUnion(const []), isEmpty);
    });

    test('fetchFollowsUnion surfaces a bridge failure', () async {
      final friends = FriendsService();
      api.stub('crateFfiIdentityIdentityFetchFollowsUnion',
          (_) => throw Exception('db down'));
      await expectLater(friends.fetchFollowsUnion(['a']), throwsException);
      expect(friends.lastError.toString(), contains('db down'));
    });

    test('AudienceFilter metadata properties', () {
      expect(AudienceFilter.all.label, 'All');
      expect(AudienceFilter.all.shortLabel, 'All');
      expect(AudienceFilter.friendsOfFriends.label, 'Friends of Friends');
      expect(AudienceFilter.friendsOfFriends.shortLabel, 'Friends of Friends');
      expect(AudienceFilter.friends.label, 'Friends');
      expect(AudienceFilter.friends.shortLabel, 'Friends');
    });

    test('matchesAudience behaves correctly with in-memory contacts and graph',
        () async {
      final friends = FriendsService();
      // Add a friend via contacts
      friends.addContact(profile('pk-friend', 'Alice'));

      // AudienceFilter.all matches anyone, even empty or stranger
      expect(friends.matchesAudience('pk-stranger', AudienceFilter.all), isTrue);
      expect(friends.matchesAudience(null, AudienceFilter.all), isTrue);

      // User's own pubkey always matches any filter
      expect(
        friends.matchesAudience('my-pk', AudienceFilter.friends,
            myPubkey: 'my-pk'),
        isTrue,
      );
      expect(
        friends.matchesAudience('my-pk', AudienceFilter.friendsOfFriends,
            myPubkey: 'my-pk'),
        isTrue,
      );

      // Friends filter matches friends only
      expect(friends.matchesAudience('pk-friend', AudienceFilter.friends),
          isTrue);
      expect(friends.matchesAudience('pk-stranger', AudienceFilter.friends),
          isFalse);
      expect(friends.matchesAudience(null, AudienceFilter.friends), isFalse);

      // Friends of friends matches friends and FOF
      expect(
          friends.matchesAudience('pk-friend', AudienceFilter.friendsOfFriends),
          isTrue);
      expect(friends.matchesAudience(
          'pk-stranger', AudienceFilter.friendsOfFriends), isFalse);
    });

    test('loadAudienceGraph and filterList filter items accurately', () async {
      final friends = FriendsService();
      api.stub('crateFfiIdentityIdentityFetchFollows', (inv) {
        final pk = api.namedArg(inv, 'pubkey');
        if (pk == 'my-pk') {
          return '["pk-friend-1"]';
        } else if (pk == 'pk-friend-1') {
          return '["pk-fof-1"]';
        }
        return '[]';
      });
      api.stub('crateFfiIdentityIdentityFetchFollowsUnion', (inv) {
        final pks = api.namedArg(inv, 'pubkeys') as List<dynamic>;
        return pks.contains('pk-friend-1') ? '["pk-fof-1"]' : '[]';
      });
      api.stubListString('crateFfiSocialSocialFriendSuggestions', const []);

      await friends.loadAudienceGraph('my-pk', force: true);

      // The friends-of-friends traversal is one batched call carrying every
      // first-degree follow, not one `fetchFollows` per follow.
      expect(api.callCount('crateFfiIdentityIdentityFetchFollowsUnion'), 1);
      final unionInv = api.callsOf('crateFfiIdentityIdentityFetchFollowsUnion').single;
      expect(api.namedArg(unionInv, 'pubkeys'), ['pk-friend-1']);
      expect(api.callCount('crateFfiIdentityIdentityFetchFollows'), 1);

      final items = [
        {'id': 1, 'pk': 'my-pk'},
        {'id': 2, 'pk': 'pk-friend-1'},
        {'id': 3, 'pk': 'pk-fof-1'},
        {'id': 4, 'pk': 'pk-stranger'},
      ];

      // All: 4 items
      final allFiltered = friends.filterList(
        items,
        AudienceFilter.all,
        (i) => i['pk'] as String,
        myPubkey: 'my-pk',
      );
      expect(allFiltered.length, 4);

      // Friends: my-pk and pk-friend-1
      final friendsFiltered = friends.filterList(
        items,
        AudienceFilter.friends,
        (i) => i['pk'] as String,
        myPubkey: 'my-pk',
      );
      expect(friendsFiltered.map((i) => i['pk']), ['my-pk', 'pk-friend-1']);

      // Friends of friends: my-pk, pk-friend-1, and pk-fof-1
      final fofFiltered = friends.filterList(
        items,
        AudienceFilter.friendsOfFriends,
        (i) => i['pk'] as String,
        myPubkey: 'my-pk',
      );
      expect(fofFiltered.map((i) => i['pk']),
          ['my-pk', 'pk-friend-1', 'pk-fof-1']);
    });

    // ─── notification precision ────────────────────────────────────────────
    //
    // `notifyListeners` on this service used to fire three times per graph load
    // (its own explicit notify, `guard`'s, and the nested `fetchSuggestions`),
    // and once more for every cache hit — with twelve screens rebuilding through
    // `filterList`, a navigation that hit the cache still rebuilt twelve whole
    // subtrees. These pin the contract that replaced it.

    void stubGraph({List<String> follows = const [], List<String> sugg = const []}) {
      api.stub('crateFfiIdentityIdentityFetchFollows',
          (_) => jsonEncode(follows));
      api.stub('crateFfiIdentityIdentityFetchFollowsUnion',
          (_) => jsonEncode(sugg));
      api.stubListString('crateFfiSocialSocialFriendSuggestions', sugg);
    }

    test('a cache-hit loadAudienceGraph notifies nobody', () async {
      final friends = FriendsService();
      stubGraph(follows: ['pk-friend-1']);

      await friends.loadAudienceGraph('my-pk', force: true);
      var notified = 0;
      friends.addListener(() => notified++);

      // Second load for the same account, already cached. This is what every
      // screen's `initState` does on navigation.
      await friends.loadAudienceGraph('my-pk');
      await friends.loadAudienceGraph('my-pk');

      expect(notified, 0,
          reason: 'a cache hit hands back the identical graph, so it is not a '
              'change; it must not rebuild the audience-filtered screens');
    });

    test('a real loadAudienceGraph notifies exactly once', () async {
      final friends = FriendsService();
      stubGraph(follows: ['pk-friend-1']);

      var notified = 0;
      friends.addListener(() => notified++);
      await friends.loadAudienceGraph('my-pk', force: true);

      expect(notified, 1,
          reason: 'the body notifies explicitly and `guard` is told not to '
              'notify on success; anything more is a duplicate rebuild');
    });

    test('a repeated fetchSuggestions with the same content is not a change',
        () async {
      final friends = FriendsService();
      // A *fresh* list per call, like the real bridge: it decodes a new list out
      // of the returned JSON every time. `api.stubListString` hands back one
      // shared instance, which would make an identity comparison accidentally
      // hold and let this exact regression through the suite.
      final handedOut = <List<String>>[];
      api.stub('crateFfiSocialSocialFriendSuggestions', (_) {
        final fresh = List<String>.of(const ['pk-1', 'pk-2']);
        handedOut.add(fresh);
        return fresh;
      });

      await friends.fetchSuggestions();
      final revision = friends.audienceRevision;

      await friends.fetchSuggestions();
      await friends.fetchSuggestions();

      expect(handedOut.length, 3);
      expect(
          handedOut
              .skip(1)
              .every((l) => !identical(l, handedOut.first)),
          isTrue,
          reason: 'guard against the stub going back to sharing one instance, '
              'which would make this test pass for the wrong reason');

      expect(friends.audienceRevision, revision,
          reason: 'the graph has identical contents, so the audience filters '
              'render identically and must not be rebuilt');
    });

    test('fetchSuggestions with different content does bump the revision',
        () async {
      final friends = FriendsService();
      api.stubListString(
          'crateFfiSocialSocialFriendSuggestions', const ['pk-1']);
      await friends.fetchSuggestions();
      final revision = friends.audienceRevision;

      api.stubListString(
          'crateFfiSocialSocialFriendSuggestions', const ['pk-1', 'pk-2']);
      await friends.fetchSuggestions();

      expect(friends.audienceRevision, greaterThan(revision));
      expect(friends.suggestions, ['pk-1', 'pk-2']);
    });

    test('audienceRevision tracks contact edits and account switch', () async {
      final friends = FriendsService();
      final start = friends.audienceRevision;

      friends.addContact(profile('pk-1', 'alice'));
      final afterAdd = friends.audienceRevision;
      expect(afterAdd, greaterThan(start));

      // A duplicate add is a no-op: it returns before mutating, so it must not
      // bump the revision either.
      friends.addContact(profile('pk-1', 'alice'));
      expect(friends.audienceRevision, afterAdd);

      friends.removeContact('pk-1');
      final afterRemove = friends.audienceRevision;
      expect(afterRemove, greaterThan(afterAdd));

      // Removing an absent contact changes nothing.
      friends.removeContact('pk-9');
      expect(friends.audienceRevision, afterRemove);

      friends.resetForAccountSwitch();
      expect(friends.audienceRevision, greaterThan(afterRemove));
    });

    test('a non-graph failure still notifies so the error banner appears',
        () async {
      final friends = FriendsService();
      api.stub('crateFfiRelationsRelationsSendFriendRequest',
          (_) => throw Exception('relay refused'));

      var notified = 0;
      friends.addListener(() => notified++);
      await expectLater(friends.sendFriendRequest('pk-9'), throwsException);

      expect(notified, 1,
          reason: 'suppressing the success notify must not suppress the error '
              'one, or a failed request would leave no banner');
      expect(friends.lastError, contains('relay refused'));
    });
  });
}
