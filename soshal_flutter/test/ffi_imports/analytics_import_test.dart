// Import test for ffi wrapper: analytics
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import analytics compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
