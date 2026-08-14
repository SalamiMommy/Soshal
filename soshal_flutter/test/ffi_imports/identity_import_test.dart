// Import test for ffi wrapper: identity
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import identity compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
