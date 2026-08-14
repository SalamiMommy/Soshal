// Generated callable ffi tests for db
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/db.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-db');
  final api = env.$1;

  test('dbPath calls crateFfiDbDbPath', () {
    api.stubString('crateFfiDbDbPath', 'stub');
    final res = dbPath();
    expect(res, 'stub');
    expect(api.callCount('crateFfiDbDbPath'), 1);
  });

  test('dbStorageStats calls crateFfiDbDbStorageStats', () {
    api.stubString('crateFfiDbDbStorageStats', 'stub');
    final res = dbStorageStats();
    expect(res, 'stub');
    expect(api.callCount('crateFfiDbDbStorageStats'), 1);
  });

}
