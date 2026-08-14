import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/protocol_handler.dart';

void main() {
  test('protocol handler wrappers forward to api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stub('crateFfiProtocolHandlerProtocolHandleRequest', (_) => Future.value(Uint8List.fromList([1, 2, 3])));
    api.stub('crateFfiProtocolHandlerProtocolGetMetadata', (_) => Future.value('{}'));

    final data = await protocolHandleRequest(scheme: 'app', host: 'h', path: '/p');
    expect(data, isA<Uint8List>());

    final meta = await protocolGetMetadata(scheme: 'app', host: 'h', path: '/p');
    expect(meta, '{}');

    expect(api.callCount('crateFfiProtocolHandlerProtocolHandleRequest'), 1);
    expect(api.callCount('crateFfiProtocolHandlerProtocolGetMetadata'), 1);
  });
}
