import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/streaming.dart';

void main() {
  test('streaming wrappers call api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stub('crateFfiStreamingStreamingStartLocalServer', (_) => Future.value(8080));
    api.stubString('crateFfiStreamingStreamingGetVideoUrl', 'http://localhost/video');
    api.stubBool('crateFfiStreamingStreamingEndLive', true);
    api.stubString('crateFfiStreamingStreamingMoqPublishObject', 'ok');

    final port = await streamingStartLocalServer();
    expect(port, 8080);

    final url = streamingGetVideoUrl(videoId: 'v', sourcePath: '/tmp');
    expect(url, 'http://localhost/video');

    final end = streamingEndLive(streamId: 's', broadcasterPubkey: 'p');
    expect(end, true);

    final moq = streamingMoqPublishObject(streamId: 's', publisherPubkey: 'p', trackId: 0, isKeyframe: false, payloadHex: '00');
    expect(moq, 'ok');

    expect(api.callCount('crateFfiStreamingStreamingStartLocalServer'), 1);
    expect(api.callCount('crateFfiStreamingStreamingGetVideoUrl'), 1);
  });
}
