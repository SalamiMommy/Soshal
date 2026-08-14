// Import test for ffi wrapper: ephemeral
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/ephemeral.dart';

void main() {
  test('import ephemeral compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
