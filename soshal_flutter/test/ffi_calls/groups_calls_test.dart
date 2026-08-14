// Generated callable ffi tests for groups
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/groups.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-groups');
  final api = env.$1;

  test('groupsFetchGroups calls String groupsFetchGroups({required String userPubkey}) => RustLib.instance.api', () {
    api.stubString('String groupsFetchGroups({required String userPubkey}) => RustLib.instance.api', 'stub');
    final res = groupsFetchGroups(userPubkey}: "x");
    expect(res, 'stub');
    expect(api.callCount('String groupsFetchGroups({required String userPubkey}) => RustLib.instance.api'), 1);
  });

}
