// Generated callable ffi tests for moderation
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/moderation.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-moderation');
  final api = env.$1;

  test('moderationDeleteReport calls bool moderationDeleteReport({required String reportId}) => RustLib.instance.api', () {
    api.stubBool('bool moderationDeleteReport({required String reportId}) => RustLib.instance.api', true);
    final res = moderationDeleteReport(reportId}: "x");
    expect(res, true);
    expect(api.callCount('bool moderationDeleteReport({required String reportId}) => RustLib.instance.api'), 1);
  });

}
