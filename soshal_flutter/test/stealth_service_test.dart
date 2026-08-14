// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/stealth_service.dart';

import './helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-stealth');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('StealthService', () {
    test('load with null setting returns empty whitelist', () async {
      final stealth = StealthService();
      api.stubString('crateFfiDbDbGetSetting', '');

      expect(await stealth.load(), isEmpty);
      expect(stealth.whitelist, isEmpty);
      expect(stealth.lastError, isNull);
      final inv = api.callsOf('crateFfiDbDbGetSetting').single;
      expect(api.namedArg(inv, 'key'), 'stealth_whitelist');
    });

    test('load parses newline-separated pubkeys and trims blanks', () async {
      final stealth = StealthService();
      api.stubString(
          'crateFfiDbDbGetSetting', '\npk-1\n pk-2 \n\npk-3\n');

      expect(await stealth.load(), ['pk-1', 'pk-2', 'pk-3']);
      expect(stealth.whitelist, ['pk-1', 'pk-2', 'pk-3']);
    });

    test('save joins items with newline and updates whitelist', () async {
      final stealth = StealthService();
      api.stubBool('crateFfiDbDbSetSetting', true);

      expect(await stealth.save(['pk-a', 'pk-b']), isTrue);
      expect(stealth.whitelist, ['pk-a', 'pk-b']);
      expect(stealth.lastError, isNull);
      final inv = api.callsOf('crateFfiDbDbSetSetting').single;
      expect(api.namedArg(inv, 'key'), 'stealth_whitelist');
      expect(api.namedArg(inv, 'value'), 'pk-a\npk-b');
    });

    test('save returning false leaves whitelist unchanged', () async {
      final stealth = StealthService();
      api.stubBool('crateFfiDbDbSetSetting', false);

      expect(await stealth.save(['pk-x']), isFalse);
      expect(stealth.whitelist, isEmpty);
    });

    test('clear deletes the stored setting', () async {
      final stealth = StealthService();
      api.stubBool('crateFfiDbDbDeleteSetting', true);

      expect(await stealth.clear(), isTrue);
      expect(stealth.lastError, isNull);
      final inv = api.callsOf('crateFfiDbDbDeleteSetting').single;
      expect(api.namedArg(inv, 'key'), 'stealth_whitelist');
    });

    test('load FFI throw sets lastError and rethrows', () async {
      final stealth = StealthService();
      api.stub(
          'crateFfiDbDbGetSetting', (_) => throw Exception('db down'));

      await expectLater(stealth.load(), throwsException);
      expect(stealth.lastError, contains('db down'));
    });

    test('save FFI throw sets lastError and rethrows', () async {
      final stealth = StealthService();
      api.stub(
          'crateFfiDbDbSetSetting', (_) => throw Exception('no write'));

      await expectLater(stealth.save(['pk-1']), throwsException);
      expect(stealth.lastError, contains('no write'));
    });
  });
}