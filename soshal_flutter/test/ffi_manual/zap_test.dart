import 'dart:core';
import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/zap.dart';

void main() {
  test('zap wrappers call api and return expected types', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stub('crateFfiZapZapParseLnurlMetadata', (_) => Future.value('{}'));
    api.stub('crateFfiZapZapConnectNwc', (_) => Future.value(true));
    api.stub('crateFfiZapZapGetTotalMsat', (_) => Future.value(BigInt.from(123)));
    api.stub('crateFfiZapZapFetchReceipts', (_) => Future.value('[]'));

    final meta = await zapParseLnurlMetadata(lnurl: 'lnurl');
    expect(meta, '{}');

    final ok = await zapConnectNwc(nwcUri: 'uri');
    expect(ok, true);

    final total = await zapGetTotalMsat(eventId: 'e');
    expect(total, BigInt.from(123));

    final receipts = await zapFetchReceipts(eventId: 'e', limit: 10);
    expect(receipts, '[]');

    expect(api.callCount('crateFfiZapZapParseLnurlMetadata'), 1);
    expect(api.callCount('crateFfiZapZapGetTotalMsat'), 1);
  });
}
