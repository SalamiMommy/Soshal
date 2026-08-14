import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/zk.dart';

void main() {
  test('zk wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiZkZkVerifyRollup', '{}');
    api.stubBool('crateFfiZkZkApplyRollup', true);

    final v = zkVerifyRollup(rollupJson: '{}');
    expect(v, '{}');

    final a = zkApplyRollup(dbPath: '/tmp/db', rollupJson: '{}');
    expect(a, true);

    expect(api.callCount('crateFfiZkZkVerifyRollup'), 1);
  });
}
