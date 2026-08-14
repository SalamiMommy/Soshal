// Generated callable ffi tests for telemetry
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/telemetry.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-telemetry');
  final api = env.$1;

  test('telemetryClear calls crateFfiTelemetryTelemetryClear', () {
    api.stubString('crateFfiTelemetryTelemetryClear', 'stub');
    final res = telemetryClear();
    expect(res, 'stub');
    expect(api.callCount('crateFfiTelemetryTelemetryClear'), 1);
  });

}
