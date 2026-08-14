import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/util.dart';

void main() {
  test('util wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiUtilUtilSha256Hex', 'dead');
    api.stubString('crateFfiUtilUtilBase64UrlEncode', 'aGVsbG8');
    api.stubString('crateFfiUtilUtilBase64UrlDecode', 'hello');
    api.stubString('crateFfiUtilUtilTruncate', 'hi');
    api.stubListString('crateFfiUtilUtilExtractHashtags', ['tag']);
    api.stubBool('crateFfiUtilUtilApplyThreadAffinity', true);

    expect(utilSha256Hex(input: 'x'), 'dead');
    expect(utilBase64UrlEncode(input: 'x'), 'aGVsbG8');
    expect(utilBase64UrlDecode(input: 'x'), 'hello');
    expect(utilTruncate(input: 'long', maxLen: BigInt.from(2)), 'hi');
    expect(utilExtractHashtags(text: '#a #b'), ['tag']);
    expect(utilApplyThreadAffinity(targetPerformance: true), true);

    expect(api.callCount('crateFfiUtilUtilSha256Hex'), 1);
  });
}
