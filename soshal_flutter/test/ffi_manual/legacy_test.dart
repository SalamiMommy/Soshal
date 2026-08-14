import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/legacy.dart';

void main() {
  test('legacy wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubBool('crateFfiLegacyLegacyHuddlePostStore', true);
    api.stubString('crateFfiLegacyLegacyHuddlePosts', '[]');
    api.stubString('crateFfiLegacyLegacyLinkPreviewGet', '{}');
    api.stub('crateFfiLegacyLegacyDiagnosticPurge', (_) => BigInt.from(0));

    final ok = legacyHuddlePostStore(huddleId: 'h', pubkey: 'p', content: 'c', expiresInSecs: BigInt.from(1));
    expect(ok, true);

    final posts = legacyHuddlePosts(huddleId: 'h', limit: 10);
    expect(posts, '[]');

    final preview = legacyLinkPreviewGet(url: 'u');
    expect(preview, '{}');

    final purged = legacyDiagnosticPurge(olderThanSecs: BigInt.from(0));
    expect(purged, BigInt.from(0));

    expect(api.callCount('crateFfiLegacyLegacyHuddlePostStore'), 1);
  });
}
