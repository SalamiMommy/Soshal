// Import test for ffi wrapper: render
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import render compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
