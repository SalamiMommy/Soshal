// Import test for ffi wrapper: legacy
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/legacy.dart';

void main() {
  test('import legacy compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
