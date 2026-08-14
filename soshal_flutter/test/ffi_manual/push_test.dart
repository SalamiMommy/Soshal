import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/push.dart';

void main() {
  test('pushRegisterToken forwards to api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubBool('crateFfiPushPushRegisterToken', true);

    final ok = pushRegisterToken(token: 'tok');
    expect(ok, true);

    expect(api.callCount('crateFfiPushPushRegisterToken'), 1);
  });
}
