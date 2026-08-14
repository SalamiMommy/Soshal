import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/audit.dart';

void main() {
  test('auditList forwards to api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiAuditAuditList', '[]');

    final res = auditList(limit: 10);
    expect(res, '[]');
    expect(api.callCount('crateFfiAuditAuditList'), 1);
  });
}
