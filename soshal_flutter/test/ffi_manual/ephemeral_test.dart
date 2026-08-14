import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/ephemeral.dart';

void main() {
  test('ephemeral wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiEphemeralEphemeralSave', 'saved');
    api.stubString('crateFfiEphemeralEphemeralGet', '{}');
    api.stubListString('crateFfiEphemeralEphemeralCleanExpired', ['a']);
    api.stubBool('crateFfiEphemeralEphemeralDelete', true);

    final id = ephemeralSave(
        messageId: 'm',
        conversationId: 'c',
        conversationType: 't',
        mediaUrl: '/u',
        mediaType: 'i',
        senderPubkey: 's',
        recipientPubkey: 'r',
        maxViews: BigInt.from(1),
        expiresAt: BigInt.from(0));
    expect(id, 'saved');

    final got = ephemeralGet(id: 'x');
    expect(got, '{}');

    final cleaned = ephemeralCleanExpired();
    expect(cleaned, ['a']);

    final del = ephemeralDelete(id: 'x');
    expect(del, true);

    expect(api.callCount('crateFfiEphemeralEphemeralSave'), 1);
  });
}
