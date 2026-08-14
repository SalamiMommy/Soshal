// Import test for ffi wrapper: scheduled
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import scheduled compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
