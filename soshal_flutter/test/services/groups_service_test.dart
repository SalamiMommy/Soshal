// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/groups_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('groups-svc');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('GroupsService', () {
    test('fetchGroups parses list and passes userPubkey', () async {
      final groups = GroupsService();
      api.stubString(
        'crateFfiGroupsGroupsFetchGroups',
        jsonEncode([
          {
            'id': 'g-1',
            'name': 'Nostr Devs',
            'description': 'builders',
            'picture': 'https://x/p.png',
            'owner': 'pk-owner',
            'members': 42,
            'is_member': true,
            'role': 'admin',
            'created_at': 1700000000,
          },
          {
            'id': 'g-2',
            'name': 'Bitcoiners',
            'description': '',
            'picture': '',
            'owner': 'pk-2',
            'members': 7,
            'is_member': false,
            'role': '',
            'created_at': 1700000001,
          },
        ]),
      );

      final result = await groups.fetchGroups('pk-me');
      expect(result.length, 2);
      expect(result.first.id, 'g-1');
      expect(result.first.name, 'Nostr Devs');
      expect(result.first.members, 42);
      expect(result.first.isMember, isTrue);
      expect(result.first.role, 'admin');
      expect(result.last.isMember, isFalse);
      expect(result.last.role, '');
      expect(groups.groups, result, reason: 'state getter updated');
      expect(groups.lastError, isNull);
      final inv = api.callsOf('crateFfiGroupsGroupsFetchGroups').single;
      expect(api.namedArg(inv, 'userPubkey'), 'pk-me');
    });

    test('getGroup sets current and parses membership flags', () async {
      final groups = GroupsService();
      api.stubString(
        'crateFfiGroupsGroupsGetGroupInfo',
        jsonEncode({
          'id': 'g-1',
          'name': 'Nostr Devs',
          'description': 'builders',
          'picture': 'https://x/p.png',
          'owner': 'pk-owner',
          'members': 42,
          'is_member': true,
          'role': 'admin',
          'created_at': 1700000000,
        }),
      );

      final group = await groups.getGroup('g-1');
      expect(group.id, 'g-1');
      expect(group.isMember, isTrue);
      expect(groups.current?.id, 'g-1', reason: 'current getter updated');
      final inv = api.callsOf('crateFfiGroupsGroupsGetGroupInfo').single;
      expect(api.namedArg(inv, 'groupId'), 'g-1');
    });

    test('loadDetailBundle populates every small facet in one call', () async {
      final groups = GroupsService();
      api.stubString(
        'crateFfiGroupsGroupsGetDetailBundle',
        jsonEncode({
          'group': {
            'id': 'g-1',
            'name': 'Nostr Devs',
            'description': 'builders',
            'picture': '',
            'owner': 'pk-owner',
            'members': 2,
            'is_member': true,
            'role': 'admin',
            'created_at': 1700000000,
          },
          'members': ['pk-a', 'pk-b'],
          'memberRoles': [
            {'pubkey': 'pk-a', 'role': 'mod'},
          ],
          'roles': [
            {
              'id': 'role-1',
              'group_id': 'g-1',
              'name': 'Mod',
              'color': '#0f0',
              'position': 1,
              'permissions': 'read',
              'created_at': 0,
            },
          ],
          'rooms': [
            {
              'id': 'room-1',
              'group_id': 'g-1',
              'name': 'general',
              'topic': '',
              'emoji': ':speech_balloon:',
              'color': '',
              'position': 0,
              'created_by': 'pk-owner',
              'created_at': 0,
            },
          ],
          'voiceChannels': [
            {
              'id': 'vc-1',
              'group_id': 'g-1',
              'name': 'stage',
              'position': 0,
              'created_by': 'pk-owner',
              'created_at': 0,
            },
          ],
        }),
      );

      await groups.loadDetailBundle('g-1');

      final inv = api.callsOf('crateFfiGroupsGroupsGetDetailBundle').single;
      expect(api.namedArg(inv, 'groupId'), 'g-1');
      // The individual facet calls must not be made any more.
      expect(api.callCount('crateFfiGroupsGroupsGetGroupInfo'), 0);
      expect(api.callCount('crateFfiGroupsGroupsGetMembers'), 0);
      expect(api.callCount('crateFfiGroupsGroupsMembersWithRoles'), 0);
      expect(api.callCount('crateFfiGroupsGroupsRolesList'), 0);
      expect(api.callCount('crateFfiGroupsGroupsRoomsList'), 0);
      expect(api.callCount('crateFfiGroupsGroupsVoiceChannelsList'), 0);

      expect(groups.current?.id, 'g-1');
      expect(groups.current?.isMember, isTrue);
      expect(groups.members, ['pk-a', 'pk-b']);
      expect(groups.memberRoles.single.pubkey, 'pk-a');
      expect(groups.memberRoles.single.role, 'mod');
      expect(groups.roles.single.name, 'Mod');
      expect(groups.rooms.single.name, 'general');
      expect(groups.voiceChannels.single.name, 'stage');
      expect(groups.lastError, isNull);
    });

    test('loadDetailBundle surfaces a bridge failure', () async {
      final groups = GroupsService();
      api.stub('crateFfiGroupsGroupsGetDetailBundle',
          (_) => throw Exception('db: NotFound'));

      await expectLater(
        groups.loadDetailBundle('missing'),
        throwsA(anything),
      );

      expect(groups.current, isNull);
      expect(groups.lastError.toString(), contains('NotFound'));
    });

    test('create returns group id and passes all named args', () async {
      final groups = GroupsService();
      api.stubString('crateFfiGroupsGroupsCreate', 'g-9');

      final id = await groups.create(
        'g-9',
        'New Group',
        'desc',
        'https://x/p.png',
        'pk-me',
        isPrivate: true,
        password: 'secretPassword123',
      );
      expect(id, 'g-9');
      final inv = api.callsOf('crateFfiGroupsGroupsCreate').single;
      expect(api.namedArg(inv, 'groupId'), 'g-9');
      expect(api.namedArg(inv, 'name'), 'New Group');
      expect(api.namedArg(inv, 'description'), 'desc');
      expect(api.namedArg(inv, 'pictureUrl'), 'https://x/p.png');
      expect(api.namedArg(inv, 'creatorPubkey'), 'pk-me');
      expect(api.namedArg(inv, 'isPrivate'), isTrue);
      expect(api.namedArg(inv, 'password'), 'secretPassword123');
    });

    test('join and leave pass groupId and pubkey and optional password', () async {
      final groups = GroupsService();
      api.stubBool('crateFfiGroupsGroupsJoin', true);
      api.stubBool('crateFfiGroupsGroupsLeave', true);

      expect(await groups.join('g-1', 'pk-me', password: 'myPassword'), isTrue);
      expect(await groups.leave('g-1', 'pk-me'), isTrue);
      final joinInv = api.callsOf('crateFfiGroupsGroupsJoin').single;
      expect(api.namedArg(joinInv, 'groupId'), 'g-1');
      expect(api.namedArg(joinInv, 'userPubkey'), 'pk-me');
      expect(api.namedArg(joinInv, 'password'), 'myPassword');
      final leaveInv = api.callsOf('crateFfiGroupsGroupsLeave').single;
      expect(api.namedArg(leaveInv, 'groupId'), 'g-1');
      expect(api.namedArg(leaveInv, 'userPubkey'), 'pk-me');
    });

    test('setPassword passes args to bridge', () async {
      final groups = GroupsService();
      api.stubBool('crateFfiGroupsGroupsSetPassword', true);

      expect(await groups.setPassword('g-1', 'newPass', 'pk-me'), isTrue);
      final setInv = api.callsOf('crateFfiGroupsGroupsSetPassword').single;
      expect(api.namedArg(setInv, 'groupId'), 'g-1');
      expect(api.namedArg(setInv, 'newPassword'), 'newPass');
      expect(api.namedArg(setInv, 'actorPubkey'), 'pk-me');
    });

    test('getMembers populates member list state', () async {
      final groups = GroupsService();
      api.stubListString(
        'crateFfiGroupsGroupsGetMembers',
        ['pk-1', 'pk-2', 'pk-me'],
      );

      final members = await groups.getMembers('g-1');
      expect(members, ['pk-1', 'pk-2', 'pk-me']);
      expect(groups.members, members, reason: 'members getter updated');
      final inv = api.callsOf('crateFfiGroupsGroupsGetMembers').single;
      expect(api.namedArg(inv, 'groupId'), 'g-1');
    });

    test('fetchMessages and roles parse into state', () async {
      final groups = GroupsService();
      api.stubString(
        'crateFfiGroupsGroupsFetchMessages',
        jsonEncode([
          {
            'id': 'm-1',
            'group_id': 'g-1',
            'sender_pubkey': 'pk-1',
            'content': 'hello',
            'created_at': 1700000000,
          }
        ]),
      );
      api.stubString(
        'crateFfiGroupsGroupsRolesList',
        jsonEncode([
          {
            'id': 'r-1',
            'group_id': 'g-1',
            'name': 'Mod',
            'color': '#e94560',
            'position': 2,
            'permissions': '["canKick"]',
            'created_at': 1700000000,
          }
        ]),
      );

      final msgs = await groups.fetchMessages('g-1');
      expect(msgs.single.content, 'hello');
      expect(msgs.single.senderPubkey, 'pk-1');
      expect(groups.messages.single.groupId, 'g-1');
      final msgInv = api.callsOf('crateFfiGroupsGroupsFetchMessages').single;
      expect(api.namedArg(msgInv, 'limit'), 100);
      expect(api.namedArg(msgInv, 'offset'), 0);

      final roles = await groups.fetchRoles('g-1');
      expect(roles.single.name, 'Mod');
      expect(roles.single.permissions, '["canKick"]');
      expect(groups.roles.single.position, 2);
      final roleInv = api.callsOf('crateFfiGroupsGroupsRolesList').single;
      expect(api.namedArg(roleInv, 'groupId'), 'g-1');
    });

    test('fetchGroups error sets lastError and rethrows', () async {
      final groups = GroupsService();
      api.stub('crateFfiGroupsGroupsFetchGroups',
          (_) => throw Exception('groups down'));

      await expectLater(groups.fetchGroups('pk-me'), throwsException);
      expect(groups.lastError, contains('groups down'));
    });

    test('create error sets lastError and rethrows', () async {
      final groups = GroupsService();
      api.stub('crateFfiGroupsGroupsCreate',
          (_) => throw Exception('create boom'));

      await expectLater(
        groups.create('g-9', 'n', 'd', '', 'pk-me'),
        throwsException,
      );
      expect(groups.lastError, contains('create boom'));
    });

    test('fetchRoomReactions parses summary and exposes via roomReactionsFor',
        () async {
      final groups = GroupsService();
      api.stubString(
        'crateFfiGroupsGroupsRoomsReactions',
        jsonEncode([
          {
            'message_id': 'm-1',
            'emoji': '👍',
            'count': 3,
            'reacted': true,
          },
          {
            'message_id': 'm-1',
            'emoji': '❤️',
            'count': 1,
            'reacted': false,
          },
          {
            'message_id': 'm-2',
            'emoji': '🔥',
            'count': 5,
            'reacted': false,
          },
        ]),
      );

      final reactions = await groups.fetchRoomReactions(
        'g-1',
        'room-alpha',
        viewerPubkey: 'pk-me',
      );

      expect(reactions.length, 3);
      expect(reactions[0].messageId, 'm-1');
      expect(reactions[0].emoji, '👍');
      expect(reactions[0].count, 3);
      expect(reactions[0].reacted, isTrue);

      final m1Reactions = groups.roomReactionsFor('m-1');
      expect(m1Reactions.length, 2);
      expect(m1Reactions.map((r) => r.emoji), containsAll(['👍', '❤️']));

      final m2Reactions = groups.roomReactionsFor('m-2');
      expect(m2Reactions.length, 1);
      expect(m2Reactions.single.emoji, '🔥');

      final inv = api.callsOf('crateFfiGroupsGroupsRoomsReactions').single;
      expect(api.namedArg(inv, 'groupId'), 'g-1');
      expect(api.namedArg(inv, 'roomId'), 'room-alpha');
      expect(api.namedArg(inv, 'viewerPubkey'), 'pk-me');

      // Verify resetForAccountSwitch clears room reactions
      groups.resetForAccountSwitch();
      expect(groups.roomReactionsFor('m-1'), isEmpty);
    });

    test('reactToRoomMessage passes args to bridge', () async {
      final groups = GroupsService();
      api.stubBool('crateFfiGroupsGroupsRoomsReact', true);

      final ok = await groups.reactToRoomMessage(
        'g-1',
        'room-alpha',
        'm-1',
        '🎉',
        'pk-me',
      );

      expect(ok, isTrue);
      final inv = api.callsOf('crateFfiGroupsGroupsRoomsReact').single;
      expect(api.namedArg(inv, 'groupId'), 'g-1');
      expect(api.namedArg(inv, 'roomId'), 'room-alpha');
      expect(api.namedArg(inv, 'messageId'), 'm-1');
      expect(api.namedArg(inv, 'emoji'), '🎉');
      expect(api.namedArg(inv, 'pubkey'), 'pk-me');
    });

    test('watchGroups and subscribeToGroups stream updates live from bridge', () async {
      final groups = GroupsService();
      final controller = StreamController<String>.broadcast();
      addTearDown(controller.close);

      api.stub('crateFfiGroupsGroupsWatchGroups', (_) => controller.stream);

      final emissions = <List<SoshalGroup>>[];
      final sub = groups.watchGroups('pk-me').listen(emissions.add);
      addTearDown(sub.cancel);

      groups.subscribeToGroups('pk-me');
      expect(groups.groupsLoading, isTrue);

      const groupPayload =
          '[{"id":"g-live","name":"Live Group","description":"desc","picture":"","owner":"pk-owner","members":5,"is_member":true,"role":"member","created_at":1700000000}]';

      controller.add(groupPayload);
      await pumpEventQueue();

      expect(emissions.length, 1);
      expect(emissions[0].first.id, 'g-live');
      expect(groups.groups.length, 1);
      expect(groups.groups.first.name, 'Live Group');
      expect(groups.groupsLoading, isFalse);

      groups.resetForAccountSwitch();
      controller.add('[]');
      await pumpEventQueue();
      expect(groups.groups.isEmpty, isTrue);
    });
  });
}

