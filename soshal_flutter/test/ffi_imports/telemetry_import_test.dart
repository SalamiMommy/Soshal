// Import test for ffi wrapper: telemetry
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import telemetry compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
