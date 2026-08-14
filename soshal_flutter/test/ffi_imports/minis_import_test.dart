// Import test for ffi wrapper: minis
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/minis.dart';

void main() {
  test('import minis compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
