// Import test for ffi wrapper: sync
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import sync compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
