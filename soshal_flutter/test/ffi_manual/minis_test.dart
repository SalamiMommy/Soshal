import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/minis.dart';

void main() {
  test('minis wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubListString('crateFfiMinisMinisFetch', ['a']);
    api.stubString('crateFfiMinisMinisWasmExecuteFilter', '{}');
    api.stubListString('crateFfiMinisMinisWasmRankFeed', ['p1']);

    final m = minisFetch();
    expect(m, ['a']);

    final out = minisWasmExecuteFilter(pluginId: 'p', text: 't', wasmBytesHex: '00');
    expect(out, '{}');

    final rank = minisWasmRankFeed(pluginId: 'p', postsJson: ['{}'], wasmBytesHex: '00');
    expect(rank, ['p1']);

    expect(api.callCount('crateFfiMinisMinisFetch'), 1);
  });
}
