import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/relations.dart';

void main() {
  test('relationsSendFriendRequest calls api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stubBool('crateFfiRelationsRelationsSendFriendRequest', true);

    final ok = relationsSendFriendRequest(pubkey: 'p');
    expect(ok, true);

    expect(api.callCount('crateFfiRelationsRelationsSendFriendRequest'), 1);
  });
}
