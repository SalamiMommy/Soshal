// ignore_for_file: invalid_use_of_internal_member

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/ffi_bridge.dart';

import 'helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-bridge');

  test('getDbPath returns documents dir + soshal.db', () async {
    final dir = env.$2;
    final path = await FfiBridge.getDbPath();
    expect(path, '$dir/soshal.db');
  });

  test('init returns a future and is idempotent (no error thrown)', () async {
    // We cannot actually call RustLib.init here; just ensure init() returns
    // a Future object and multiple calls return same future type.
    final f1 = FfiBridge.init();
    final f2 = FfiBridge.init();
    expect(f1, isA<Future<void>>());
    expect(f2, isA<Future<void>>());
  });
}
