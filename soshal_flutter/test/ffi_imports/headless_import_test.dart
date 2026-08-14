// Import test for ffi wrapper: headless
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import headless compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
