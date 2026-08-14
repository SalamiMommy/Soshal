import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/chatrandom.dart';

void main() {
  test('chatrandom wrappers call api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stubString('crateFfiChatrandomChatrandomAvailableContent', '[]');
    api.stub('crateFfiChatrandomChatrandomSend', (_) => Future.value('evt'));
    api.stub('crateFfiChatrandomChatrandomFetch', (_) => Future.value('[]'));

    final avail = chatrandomAvailableContent(interests: ['a'], mediaType: 'img', mode: 'any');
    expect(avail, '[]');

    final s = await chatrandomSend(requestType: 'request', peers: ['p'], contentJson: '{}');
    expect(s, 'evt');

    final f = await chatrandomFetch(myPubkey: 'me', author: null, limit: BigInt.from(1));
    expect(f, '[]');

    expect(api.callCount('crateFfiChatrandomChatrandomAvailableContent'), 1);
  });
}
