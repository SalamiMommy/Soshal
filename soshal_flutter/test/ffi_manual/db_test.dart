import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/db.dart';

void main() {
  test('db wrappers forward to api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiDbDbInit', 'ok');
    api.stubString('crateFfiDbDbPath', '/tmp/db');
    api.stubString('crateFfiDbDbQueryRaw', '[]');
    api.stub('crateFfiDbDbExecuteRaw', (_) => BigInt.from(2));
    api.stubBool('crateFfiDbDbSetSetting', true);

    final init = dbInit(dbPath: '/tmp/db');
    expect(init, 'ok');

    final path = dbPath();
    expect(path, '/tmp/db');

    final rows = dbQueryRaw(sql: 'SELECT 1');
    expect(rows, '[]');

    final affected = dbExecuteRaw(sql: 'UPDATE');
    expect(affected, BigInt.from(2));

    final s = dbSetSetting(key: 'k', value: 'v');
    expect(s, true);

    expect(api.callCount('crateFfiDbDbInit'), 1);
    expect(api.callCount('crateFfiDbDbQueryRaw'), 1);
  });
}
