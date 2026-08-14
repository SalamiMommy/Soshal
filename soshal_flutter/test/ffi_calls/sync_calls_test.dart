// Generated callable ffi tests for sync
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/sync.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-sync');
  final api = env.$1;

  test('syncEvents calls crateFfiSyncSyncEvents', () {
    api.stubString('crateFfiSyncSyncEvents', 'stub');
    final res = syncEvents();
    expect(res, 'stub');
    expect(api.callCount('crateFfiSyncSyncEvents'), 1);
  });

  test('syncStop calls crateFfiSyncSyncStop', () {
    api.stubBool('crateFfiSyncSyncStop', true);
    final res = syncStop();
    expect(res, true);
    expect(api.callCount('crateFfiSyncSyncStop'), 1);
  });

  test('syncRunning calls crateFfiSyncSyncRunning', () {
    api.stubBool('crateFfiSyncSyncRunning', true);
    final res = syncRunning();
    expect(res, true);
    expect(api.callCount('crateFfiSyncSyncRunning'), 1);
  });

}
