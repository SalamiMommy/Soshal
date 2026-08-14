// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/groups_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

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

    test('create returns group id and passes all named args', () async {
      final groups = GroupsService();
      api.stubString('crateFfiGroupsGroupsCreate', 'g-9');

      final id = await groups.create(
        'g-9',
        'New Group',
        'desc',
        'https://x/p.png',
        'pk-me',
      );
      expect(id, 'g-9');
      final inv = api.callsOf('crateFfiGroupsGroupsCreate').single;
      expect(api.namedArg(inv, 'groupId'), 'g-9');
      expect(api.namedArg(inv, 'name'), 'New Group');
      expect(api.namedArg(inv, 'description'), 'desc');
      expect(api.namedArg(inv, 'pictureUrl'), 'https://x/p.png');
      expect(api.namedArg(inv, 'creatorPubkey'), 'pk-me');
    });

    test('join and leave pass groupId and pubkey', () async {
      final groups = GroupsService();
      api.stubBool('crateFfiGroupsGroupsJoin', true);
      api.stubBool('crateFfiGroupsGroupsLeave', true);

      expect(await groups.join('g-1', 'pk-me'), isTrue);
      expect(await groups.leave('g-1', 'pk-me'), isTrue);
      final joinInv = api.callsOf('crateFfiGroupsGroupsJoin').single;
      expect(api.namedArg(joinInv, 'groupId'), 'g-1');
      expect(api.namedArg(joinInv, 'userPubkey'), 'pk-me');
      final leaveInv = api.callsOf('crateFfiGroupsGroupsLeave').single;
      expect(api.namedArg(leaveInv, 'groupId'), 'g-1');
      expect(api.namedArg(leaveInv, 'userPubkey'), 'pk-me');
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
  });
}
