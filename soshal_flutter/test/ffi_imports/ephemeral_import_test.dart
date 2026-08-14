// Import test for ffi wrapper: ephemeral
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import ephemeral compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
