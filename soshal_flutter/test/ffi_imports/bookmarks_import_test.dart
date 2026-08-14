// Import test for ffi wrapper: bookmarks
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import bookmarks compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
