import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/search.dart';

void main() {
  test('search wrappers call expected api methods', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiSearchSearchPosts', '[]');
    api.stubListString('crateFfiSearchSearchHashtags', ['tag1']);
    api.stub('crateFfiSearchSearchRemoteGlobal', (_) => Future.value('[]'));
    api.stubBool('crateFfiSearchSearchIndexPost', true);

    final posts = searchPosts(query: 'x', limit: 5);
    expect(posts, '[]');

    final tags = searchHashtags(query: 'x', limit: 3);
    expect(tags, ['tag1']);

    final remote = await searchRemoteGlobal(query: 'q', limit: BigInt.from(1), relaysJson: '[]');
    expect(remote, '[]');

    final indexed = searchIndexPost(eventId: 'e', pubkey: 'p', content: 'c', kind: 1);
    expect(indexed, true);

    expect(api.callCount('crateFfiSearchSearchPosts'), 1);
    expect(api.callCount('crateFfiSearchSearchHashtags'), 1);
    expect(api.callCount('crateFfiSearchSearchRemoteGlobal'), 1);
    expect(api.callCount('crateFfiSearchSearchIndexPost'), 1);
  });
}
