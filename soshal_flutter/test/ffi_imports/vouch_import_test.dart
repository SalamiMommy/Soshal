// Import test for ffi wrapper: vouch
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/vouch.dart';

void main() {
  test('import vouch compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
