// Manual ffi tests for messaging
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/messaging.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-messaging-manual');
  final api = env.$1;

  test('messagingSendDm returns signed JSON (Future)', () async {
    api.stub('crateFfiMessagingMessagingSendDm', (_) => Future.value('signed'));
    final res = await messagingSendDm(content: 'hi', recipientPubkey: 'pk');
    expect(res, 'signed');
    expect(api.callCount('crateFfiMessagingMessagingSendDm'), 1);
  });

  test('messagingDecryptDm and messagingFetchDms', () {
    api.stubString('crateFfiMessagingMessagingDecryptDm', 'plain');
    api.stubString('crateFfiMessagingMessagingFetchDms', '[]');
    final dec = messagingDecryptDm(payload: 'p', senderPubkey: 's');
    final dms = messagingFetchDms(withPubkey: 's', limit: 10);
    expect(dec, 'plain');
    expect(dms, '[]');
    expect(api.callCount('crateFfiMessagingMessagingDecryptDm'), 1);
    expect(api.callCount('crateFfiMessagingMessagingFetchDms'), 1);
  });

  test('messagingFetchConversations and messagingStoreDm', () {
    api.stubListString('crateFfiMessagingMessagingFetchConversations', ['a']);
    api.stubBool('crateFfiMessagingMessagingStoreDm', true);
    final convs = messagingFetchConversations(pubkey: 'me');
    expect(convs, ['a']);
    final stored = messagingStoreDm(id: 'id', sender: 's', recipient: 'r', content: 'c', createdAt: BigInt.from(1), tagsJson: '[]');
    expect(stored, true);
    expect(api.callCount('crateFfiMessagingMessagingFetchConversations'), 1);
    expect(api.callCount('crateFfiMessagingMessagingStoreDm'), 1);
  });

  test('messagingSendGroupDm calls group send', () {
    api.stubString('crateFfiMessagingMessagingSendGroupDm', 'g');
    final res = messagingSendGroupDm(content: 'x', groupId: 'g', participantPubkeysJson: '[]');
    expect(res, 'g');
    expect(api.callCount('crateFfiMessagingMessagingSendGroupDm'), 1);
  });
}
