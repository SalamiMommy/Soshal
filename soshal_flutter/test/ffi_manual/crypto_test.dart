import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/crypto.dart';

void main() {
  test('crypto wrappers call api and return expected shapes', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiCryptoCryptoSha256Hex', 'deadbeef');
    api.stub('crateFfiCryptoCryptoSha256', (_) => Uint8List.fromList([1, 2, 3]));
    api.stub('crateFfiCryptoCryptoHmacSha256', (_) => Uint8List.fromList([4, 5]));
    api.stub('crateFfiCryptoCryptoPqcKemKeygen', (_) => Future.value('{}'));
    api.stubString('crateFfiCryptoCryptoHkdfExpand', 'aa');
    api.stubBool('crateFfiCryptoCryptoZeroize', true);

    final hex = cryptoSha256Hex(input: 'x');
    expect(hex, 'deadbeef');

    final bytes = cryptoSha256(input: [1]);
    expect(bytes, isA<Uint8List>());

    final hmac = cryptoHmacSha256(key: [1], message: [2]);
    expect(hmac, isA<Uint8List>());

    final pq = await cryptoPqcKemKeygen();
    expect(pq, '{}');

    final hk = cryptoHkdfExpand(ikm: [1], salt: [1], info: [1], len: BigInt.from(16));
    expect(hk, 'aa');

    final z = cryptoZeroize(data: [0]);
    expect(z, true);

    expect(api.callCount('crateFfiCryptoCryptoSha256Hex'), 1);
    expect(api.callCount('crateFfiCryptoCryptoPqcKemKeygen'), 1);
  });
}
