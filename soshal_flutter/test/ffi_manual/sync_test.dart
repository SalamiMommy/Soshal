import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/sync.dart';

void main() {
  test('sync wrappers forward to api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stub('crateFfiSyncSyncStart', (_) => Future.value('started'));
    api.stub('crateFfiSyncSyncStop', (_) => Future.value(true));
    api.stubBool('crateFfiSyncSyncRunning', false);
    api.stubString('crateFfiSyncSyncEnqueueOutbox', 'queued');

    final s = await syncStart(relaysJson: '[]');
    expect(s, 'started');

    final stopped = await syncStop();
    expect(stopped, true);

    final running = syncRunning();
    expect(running, false);

    final q = syncEnqueueOutbox(actionType: 'post', payloadJson: '{}');
    expect(q, 'queued');

    expect(api.callCount('crateFfiSyncSyncStart'), 1);
    expect(api.callCount('crateFfiSyncSyncStop'), 1);
    expect(api.callCount('crateFfiSyncSyncEnqueueOutbox'), 1);
  });
}
