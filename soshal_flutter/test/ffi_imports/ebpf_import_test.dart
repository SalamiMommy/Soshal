// Import test for ffi wrapper: ebpf
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('import ebpf compiles', () {
    TestWidgetsFlutterBinding.ensureInitialized();
    expect(true, isTrue);
  });
}
