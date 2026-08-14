// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/signer_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-signer');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('SignerService', () {
    test('pubkey/isLocked/lock forward to api', () async {
      api.stubString('crateFfiSignerSignerPubkey', 'abcd');
      api.stubBool('crateFfiSignerSignerIsLocked', true);
      api.stubBool('crateFfiSignerSignerLock', true);

      final s = SignerService();
      final pk = await s.pubkey();
      final locked = await s.isLocked();
      final lockedAfter = await s.lock();

      expect(pk, 'abcd');
      expect(locked, true);
      expect(lockedAfter, true);
    });

    test('keyring ops call backend', () async {
      api.stubBool('crateFfiSignerSignerSaveToKeyring', true);
      api.stubBool('crateFfiSignerSignerUnlockFromKeyring', true);
      api.stubBool('crateFfiSignerSignerRemoveFromKeyring', true);

      final s = SignerService();
      expect(await s.saveToKeyring('pk1'), true);
      expect(await s.unlockFromKeyring('pk1'), true);
      expect(await s.removeFromKeyring('pk1'), true);

      final inv = api.callsOf('crateFfiSignerSignerSaveToKeyring').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk1');
    });

    test('signText returns signature string', () async {
      api.stubString('crateFfiSignerSignerSignText', 'sig123');

      final s = SignerService();
      final sig = await s.signText('hello');
      expect(sig, 'sig123');

      final inv = api.callsOf('crateFfiSignerSignerSignText').single;
      expect(api.namedArg(inv, 'message'), 'hello');
    });
  });
}
