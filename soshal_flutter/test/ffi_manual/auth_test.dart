// Manual ffi tests for auth
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/auth.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-auth-manual');
  final api = env.$1;

  test('generate and validate mnemonic/keypair', () async {
    api.stubString('crateFfiAuthAuthGenerateKeypair', 'kp');
    api.stubString('crateFfiAuthAuthGenerateMnemonic', 'mn');
    api.stubBool('crateFfiAuthAuthValidateMnemonic', true);
    api.stub('crateFfiAuthAuthRestoreFromMnemonic', (_) => Future.value('restored'));
    final kp = authGenerateKeypair();
    final mn = authGenerateMnemonic();
    final ok = authValidateMnemonic(mnemonic: 'm');
    final restored = await authRestoreFromMnemonic(mnemonic: 'm', passphrase: 'p');
    expect(kp, 'kp');
    expect(mn, 'mn');
    expect(ok, true);
    expect(restored, 'restored');
  });

  test('pubkey/npub encode/decode', () {
    api.stubString('crateFfiAuthAuthPublicKeyFromNsec', 'pub');
    api.stubString('crateFfiAuthAuthNpubEncode', 'npub');
    api.stubString('crateFfiAuthAuthNpubDecode', 'pub');
    final p = authPublicKeyFromNsec(nsec: 'n');
    final np = authNpubEncode(publicKey: 'pub');
    final dec = authNpubDecode(npub: 'npub');
    expect(p, 'pub');
    expect(np, 'npub');
    expect(dec, 'pub');
  });
}
