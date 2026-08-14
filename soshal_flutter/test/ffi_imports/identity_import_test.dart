// Import test for ffi wrapper: identity
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/identity.dart';

void main() {
  test('import identity compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
