// Import test for ffi wrapper: calls
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/calls.dart';

void main() {
  test('import calls compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
