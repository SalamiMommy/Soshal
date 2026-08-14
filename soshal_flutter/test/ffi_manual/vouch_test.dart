import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/vouch.dart';

void main() {
  test('vouch wrappers forward to api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stub('crateFfiVouchVouchPublish', (_) => Future.value('evt1'));
    api.stub('crateFfiVouchVouchFetch', (_) => Future.value('[]'));

    final pub = await vouchPublish(targetPubkey: 'p', content: 'c');
    expect(pub, 'evt1');

    final fetch = await vouchFetch(targetPubkey: 'p');
    expect(fetch, '[]');

    expect(api.callCount('crateFfiVouchVouchPublish'), 1);
    expect(api.callCount('crateFfiVouchVouchFetch'), 1);
  });
}
