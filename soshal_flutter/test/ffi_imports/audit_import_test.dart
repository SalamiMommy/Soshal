// Import test for ffi wrapper: audit
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/audit.dart';

void main() {
  test('import audit compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
