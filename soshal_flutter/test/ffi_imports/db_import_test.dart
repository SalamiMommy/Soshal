// Import test for ffi wrapper: db
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/db.dart';

void main() {
  test('import db compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
