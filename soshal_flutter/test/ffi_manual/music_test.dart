import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/music.dart';

void main() {
  test('music wrappers call api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stub('crateFfiMusicMusicPublish', (_) => Future.value('evt'));
    api.stub('crateFfiMusicMusicFetch', (_) => Future.value('[]'));
    api.stub('crateFfiMusicMusicShareToFeed', (_) => Future.value('ok'));
    api.stub('crateFfiMusicMusicComment', (_) => Future.value('{}'));
    api.stub('crateFfiMusicMusicComments', (_) => Future.value('[]'));

    final pub = await musicPublish(audioUrl: 'https://x', title: 't', thumbnail: null, hashtags: [], audience: null);
    expect(pub, 'evt');

    final f = await musicFetch(limit: BigInt.from(1));
    expect(f, '[]');

    final share = await musicShareToFeed(trackId: 'id', trackPubkey: 'pk', message: 'm', hashtags: []);
    expect(share, 'ok');

    expect(api.callCount('crateFfiMusicMusicPublish'), 1);
  });
}
