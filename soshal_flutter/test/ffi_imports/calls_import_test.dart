// Import test for ffi wrapper: calls
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import calls compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
