import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/telemetry.dart';

void main() {
  test('telemetry wrappers call api and return expected types', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stub('crateFfiTelemetryTelemetryInit', (_) => null);
    api.stub('crateFfiTelemetryTelemetryDumpEncrypted', (_) => Uint8List.fromList([1, 2]));
    api.stubString('crateFfiTelemetryTelemetryInfoJson', '{"entries":0}');
    api.stubBool('crateFfiTelemetryTelemetryIsSealed', false);
    api.stubString('crateFfiTelemetryTelemetryReadAllJson', '[]');

    telemetryInit(path: tmp, capacityMb: 1);
    final dump = telemetryDumpEncrypted();
    expect(dump, isA<Uint8List>());

    final info = telemetryInfoJson();
    expect(info, '{"entries":0}');

    final sealed = telemetryIsSealed();
    expect(sealed, false);

    final all = telemetryReadAllJson();
    expect(all, '[]');

    expect(api.callCount('crateFfiTelemetryTelemetryInit'), 1);
    expect(api.callCount('crateFfiTelemetryTelemetryDumpEncrypted'), 1);
  });
}
