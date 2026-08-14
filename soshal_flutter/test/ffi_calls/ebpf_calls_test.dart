// Generated callable ffi tests for ebpf
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/ebpf.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-ebpf');
  final api = env.$1;

  test('ebpfGetStats calls crateFfiEbpfEbpfGetStats', () {
    api.stubString('crateFfiEbpfEbpfGetStats', 'stub');
    final res = ebpfGetStats();
    expect(res, 'stub');
    expect(api.callCount('crateFfiEbpfEbpfGetStats'), 1);
  });

}
