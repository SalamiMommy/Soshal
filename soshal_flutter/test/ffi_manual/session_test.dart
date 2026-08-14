// Manual ffi tests for session
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/session.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-session-manual');
  final api = env.$1;

  test('sessionLoad and sessionSave', () {
    api.stubString('crateFfiSessionSessionLoad', 'loaded');
    api.stubBool('crateFfiSessionSessionSave', true);
    final s = sessionLoad(dbPath: '/tmp/db');
    final saved = sessionSave(dbPath: '/tmp/db', sessionData: '{}');
    expect(s, 'loaded');
    expect(saved, true);
    expect(api.callCount('crateFfiSessionSessionLoad'), 1);
    expect(api.callCount('crateFfiSessionSessionSave'), 1);
  });

  test('account management', () {
    api.stubBool('crateFfiSessionSessionAddAccount', true);
    api.stubBool('crateFfiSessionSessionSwitchAccount', true);
    api.stubString('crateFfiSessionSessionGetActive', 'me');
    api.stubString('crateFfiSessionSessionListAccounts', '[]');
    final added = sessionAddAccount(pubkey: 'p', npub: 'n', relaysJson: '[]');
    final sw = sessionSwitchAccount(pubkey: 'p');
    final active = sessionGetActive();
    final list = sessionListAccounts();
    expect(added, true);
    expect(sw, true);
    expect(active, 'me');
    expect(list, '[]');
    expect(api.callCount('crateFfiSessionSessionAddAccount'), 1);
    expect(api.callCount('crateFfiSessionSessionSwitchAccount'), 1);
  });

  test('sessionRegisterPushToken', () {
    api.stubBool('crateFfiSessionSessionRegisterPushToken', true);
    final r = sessionRegisterPushToken(token: 't');
    expect(r, true);
    expect(api.callCount('crateFfiSessionSessionRegisterPushToken'), 1);
  });
}
