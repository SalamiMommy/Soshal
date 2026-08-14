// Manual ffi tests for signer
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/signer.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-signer-manual');
  final api = env.$1;

  test('signerUnlock calls crateFfiSignerSignerUnlock', () {
    api.stubString('crateFfiSignerSignerUnlock', 'pubkey');
    final res = signerUnlock(secret: 'nsec1...');
    expect(res, 'pubkey');
    expect(api.callCount('crateFfiSignerSignerUnlock'), 1);
  });

  test('signerLock and signerIsLocked call respective methods', () {
    api.stubBool('crateFfiSignerSignerLock', true);
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    final lockedRes = signerLock();
    final isLocked = signerIsLocked();
    expect(lockedRes, true);
    expect(isLocked, false);
    expect(api.callCount('crateFfiSignerSignerLock'), 1);
    expect(api.callCount('crateFfiSignerSignerIsLocked'), 1);
  });

  test('signerSchnorrSign and signerSignText call signing methods', () {
    api.stubString('crateFfiSignerSignerSchnorrSign', 'sighex');
    api.stubString('crateFfiSignerSignerSignText', 'sigtext');
    final res1 = signerSchnorrSign(messageHex: 'aa');
    final res2 = signerSignText(message: 'hello');
    expect(res1, 'sighex');
    expect(res2, 'sigtext');
    expect(api.callCount('crateFfiSignerSignerSchnorrSign'), 1);
    expect(api.callCount('crateFfiSignerSignerSignText'), 1);
  });
}
