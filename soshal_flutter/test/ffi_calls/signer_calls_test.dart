// Generated callable ffi tests for signer
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/signer.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-signer');
  final api = env.$1;

  test('signerLock calls crateFfiSignerSignerLock', () {
    api.stubBool('crateFfiSignerSignerLock', true);
    final res = signerLock();
    expect(res, true);
    expect(api.callCount('crateFfiSignerSignerLock'), 1);
  });

  test('signerIsLocked calls crateFfiSignerSignerIsLocked', () {
    api.stubBool('crateFfiSignerSignerIsLocked', true);
    final res = signerIsLocked();
    expect(res, true);
    expect(api.callCount('crateFfiSignerSignerIsLocked'), 1);
  });

  test('signerPubkey calls crateFfiSignerSignerPubkey', () {
    api.stubString('crateFfiSignerSignerPubkey', 'stub');
    final res = signerPubkey();
    expect(res, 'stub');
    expect(api.callCount('crateFfiSignerSignerPubkey'), 1);
  });

  test('signerSchnorrSign calls String signerSchnorrSign({required String messageHex}) => RustLib.instance.api', () {
    api.stubString('String signerSchnorrSign({required String messageHex}) => RustLib.instance.api', 'stub');
    final res = signerSchnorrSign(messageHex}: "x");
    expect(res, 'stub');
    expect(api.callCount('String signerSchnorrSign({required String messageHex}) => RustLib.instance.api'), 1);
  });

}
