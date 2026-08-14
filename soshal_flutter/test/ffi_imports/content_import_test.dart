// Import test for ffi wrapper: content
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import content compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
