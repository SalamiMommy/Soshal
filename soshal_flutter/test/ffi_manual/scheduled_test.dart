import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/scheduled.dart';

void main() {
  test('scheduled wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiScheduledScheduledCreate', '{}');
    api.stubString('crateFfiScheduledScheduledList', '[]');
    api.stubBool('crateFfiScheduledScheduledDelete', true);

    final c = scheduledCreate(pubkey: 'p', content: 'c', scheduledAt: BigInt.from(1), hashtags: ['a']);
    expect(c, '{}');

    final list = scheduledList(pubkey: 'p');
    expect(list, '[]');

    final d = scheduledDelete(id: 'id');
    expect(d, true);

    expect(api.callCount('crateFfiScheduledScheduledCreate'), 1);
  });
}
