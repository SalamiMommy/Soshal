import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/headless.dart';

void main() {
  test('headless backgroundSyncTask forwards to api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stubInt('crateFfiHeadlessBackgroundSyncTask', 0);

    final res = backgroundSyncTask(dbPath: '/tmp/db');
    expect(res, 0);

    expect(api.callCount('crateFfiHeadlessBackgroundSyncTask'), 1);
  });
}