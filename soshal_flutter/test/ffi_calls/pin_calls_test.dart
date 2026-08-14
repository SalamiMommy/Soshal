// Generated callable ffi tests for pin
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/pin.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-pin');
  final api = env.$1;

  test('pinHas calls crateFfiPinPinHas', () {
    api.stubBool('crateFfiPinPinHas', true);
    final res = pinHas();
    expect(res, true);
    expect(api.callCount('crateFfiPinPinHas'), 1);
  });

  test('pinLockoutState calls crateFfiPinPinLockoutState', () {
    api.stubString('crateFfiPinPinLockoutState', 'stub');
    final res = pinLockoutState();
    expect(res, 'stub');
    expect(api.callCount('crateFfiPinPinLockoutState'), 1);
  });

}
