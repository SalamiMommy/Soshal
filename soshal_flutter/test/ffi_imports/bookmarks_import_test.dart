// Import test for ffi wrapper: bookmarks
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/bookmarks.dart';

void main() {
  test('import bookmarks compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
