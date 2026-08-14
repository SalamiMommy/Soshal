import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/frb_generated.web.dart';

void main() {
  test('frb_generated.web exports expected symbols', () {
    // Ensure generated web bindings expose core types used by the app and tests.
    expect(RustLibWire, isNotNull);
    expect(RustLibApiImplPlatform, isNotNull);
  });
}
