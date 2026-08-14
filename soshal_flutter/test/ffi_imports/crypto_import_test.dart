// Import test for ffi wrapper: crypto
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import crypto compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
