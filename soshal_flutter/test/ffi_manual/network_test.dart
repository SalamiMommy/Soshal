// Manual ffi tests for network
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/network.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-network-manual');
  final api = env.$1;

  test('networkInitRelays and networkAddRelay', () async {
    api.stub('crateFfiNetworkNetworkInitRelays', (_) => Future.value('ok'));
    api.stub('crateFfiNetworkNetworkAddRelay', (_) => Future.value(true));
    final init = await networkInitRelays(relayUrls: ['r']);
    final added = await networkAddRelay(url: 'r');
    expect(init, 'ok');
    expect(added, true);
    expect(api.callCount('crateFfiNetworkNetworkInitRelays'), 1);
    expect(api.callCount('crateFfiNetworkNetworkAddRelay'), 1);
  });

  test('networkGetRelayStatus and networkPublishEvent', () async {
    api.stubString('crateFfiNetworkNetworkGetRelayStatus', '[{"url":"r"}]');
    api.stub('crateFfiNetworkNetworkPublishEvent', (_) => Future.value(3));
    final status = networkGetRelayStatus();
    final published = await networkPublishEvent(eventJson: '{}');
    expect(status, '[{"url":"r"}]');
    expect(published, 3);
    expect(api.callCount('crateFfiNetworkNetworkGetRelayStatus'), 1);
    expect(api.callCount('crateFfiNetworkNetworkPublishEvent'), 1);
  });

  test('networkFetchHttp3 returns HttpResponseDto', () async {
    api.stub('crateFfiNetworkNetworkFetchHttp3', (_) => Future.value(HttpResponseDto(status: 200, body: Uint8List.fromList([1]))));
    final res = await networkFetchHttp3(url: 'http://x', method: 'GET', headersJson: '[]');
    expect(res, isA<HttpResponseDto>());
    expect(res.status, 200);
    expect(api.callCount('crateFfiNetworkNetworkFetchHttp3'), 1);
  });
}
