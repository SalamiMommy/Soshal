import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/ebpf.dart';

void main() {
  test('ebpf wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stubBool('crateFfiEbpfEbpfBlockIp', true);
    api.stubBool('crateFfiEbpfEbpfUnblockIp', true);
    api.stubString('crateFfiEbpfEbpfGetStats', '{}');

    final b = ebpfBlockIp(ip: '1.2.3.4');
    expect(b, true);

    final ub = ebpfUnblockIp(ip: '1.2.3.4');
    expect(ub, true);

    final s = ebpfGetStats();
    expect(s, '{}');

    expect(api.callCount('crateFfiEbpfEbpfBlockIp'), 1);
  });
}
