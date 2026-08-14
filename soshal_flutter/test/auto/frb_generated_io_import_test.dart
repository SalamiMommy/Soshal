import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/frb_generated.io.dart';

void main() {
  test('frb_generated.io exports expected symbols', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    // Ensure generated io bindings expose core types used by the app and tests.
    expect(RustLibWire, isNotNull);
    expect(RustLibApiImplPlatform, isNotNull);
  });
}
