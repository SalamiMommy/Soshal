// Generated callable ffi tests for minis
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/minis.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-minis');
  final api = env.$1;

  test('minisFetch calls crateFfiMinisMinisFetch', () {
    api.stubListString('crateFfiMinisMinisFetch', ['stub']);
    final res = minisFetch();
    expect(res, ['stub']);
    expect(api.callCount('crateFfiMinisMinisFetch'), 1);
  });

}
