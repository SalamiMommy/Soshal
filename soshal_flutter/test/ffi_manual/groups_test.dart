// Manual ffi tests for groups
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/groups.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-groups-manual');
  final api = env.$1;

  test('fetch/get/join/leave/members/create', () {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[]');
    api.stubString('crateFfiGroupsGroupsGetGroupInfo', '{}');
    api.stubListString('crateFfiGroupsGroupsGetMembers', ['a']);
    api.stubBool('crateFfiGroupsGroupsJoin', true);
    api.stubBool('crateFfiGroupsGroupsLeave', true);
    api.stubString('crateFfiGroupsGroupsPostMessage', '{}');
    api.stubString('crateFfiGroupsGroupsFetchMessages', '[]');
    api.stubString('crateFfiGroupsGroupsCreate', 'gid');

    final groups = groupsFetchGroups(userPubkey: 'u');
    final info = groupsGetGroupInfo(groupId: 'g');
    final members = groupsGetMembers(groupId: 'g');
    final j = groupsJoin(groupId: 'g', userPubkey: 'u');
    final l = groupsLeave(groupId: 'g', userPubkey: 'u');
    final pm = groupsPostMessage(groupId: 'g', content: 'c');
    final msgs = groupsFetchMessages(groupId: 'g', limit: 10, offset: 0);
    final created = groupsCreate(groupId: 'g', name: 'n', description: '', pictureUrl: '', creatorPubkey: 'u');

    expect(groups, '[]');
    expect(info, '{}');
    expect(members, ['a']);
    expect(j, true);
    expect(l, true);
    expect(pm, '{}');
    expect(msgs, '[]');
    expect(created, 'gid');
    expect(api.callCount('crateFfiGroupsGroupsFetchGroups'), 1);
  });
}
