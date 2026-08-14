import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/pin.dart';

void main() {
  test('pin wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubBool('crateFfiPinPinSet', true);
    api.stubBool('crateFfiPinPinHas', false);
    api.stubBool('crateFfiPinPinVerify', true);
    api.stubBool('crateFfiPinPinClear', true);
    api.stubString('crateFfiPinPinLockoutState', '{}');

    final s = pinSet(pin: '1234');
    expect(s, true);

    final has = pinHas();
    expect(has, false);

    final v = pinVerify(pin: '1234');
    expect(v, true);

    final clr = pinClear(pin: '1234');
    expect(clr, true);

    final state = pinLockoutState();
    expect(state, '{}');

    expect(api.callCount('crateFfiPinPinSet'), 1);
  });
}
