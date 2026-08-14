// Manual ffi tests for p2p
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/p2p.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-p2p-manual');
  final api = env.$1;

  test('p2PLanServerStart and Port/Stop', () {
    api.stubInt('crateFfiP2PP2PLanServerStart', 1234);
    api.stubInt('crateFfiP2PP2PLanServerPort', 1234);
    api.stubBool('crateFfiP2PP2PLanServerStop', true);
    final port = p2PLanServerStart(storeRoot: '/tmp');
    final got = p2PLanServerPort();
    final stopped = p2PLanServerStop();
    expect(port, 1234);
    expect(got, 1234);
    expect(stopped, true);
    expect(api.callCount('crateFfiP2PP2PLanServerStart'), 1);
  });

  test('p2PQuicFetchChunk and p2PEncode/Decode fountain', () {
    api.stub('crateFfiP2PP2PQuicFetchChunk', (_) => Uint8List.fromList([1,2,3]));
    api.stubString('crateFfiP2PP2PEncodeFountainPayload', 'manifest');
    api.stub('crateFfiP2PP2PDecodeFountainPayload', (_) => Uint8List.fromList([9,9]));
    final chunk = p2PQuicFetchChunk(addr: '10.0.0.1', hash: 'h', offset: BigInt.from(0), length: BigInt.from(10));
    final manifest = p2PEncodeFountainPayload(data: [1,2,3], redundancyRatio: 0.5);
    final decoded = p2PDecodeFountainPayload(manifestJson: 'm', packetsB64Json: '[]');
    expect(chunk, isA<Uint8List>());
    expect(manifest, 'manifest');
    expect(decoded, isA<Uint8List>());
    expect(api.callCount('crateFfiP2PP2PQuicFetchChunk'), 1);
  });
}
