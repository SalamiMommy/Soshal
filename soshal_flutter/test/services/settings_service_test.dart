// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/settings_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-settings');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('SettingsService', () {
    test('getSetting reads key from backend', () {
      const value = 'setting_value';
      api.stubString('crateFfiDbDbGetSetting', value);

      final settings = SettingsService();
      final result = settings.getSetting('my_key');

      expect(result, value);
      final inv = api.callsOf('crateFfiDbDbGetSetting').single;
      expect(api.namedArg(inv, 'key'), 'my_key');
    });

    test('getSetting returns empty string when unset', () {
      api.stubString('crateFfiDbDbGetSetting', '');

      final settings = SettingsService();
      final result = settings.getSetting('unset_key');

      expect(result, '');
    });

    test('storageStats parses backend JSON response', () {
      const statsJson =
          '[{"table":"posts","rows":100},{"table":"events","rows":200},{"__db_file__":"size","size_bytes":1000000}]';
      api.stubString('crateFfiDbDbStorageStats', statsJson);

      final settings = SettingsService();
      final stats = settings.storageStats();

      expect(stats.length, 3);
      expect(stats[0]['table'], 'posts');
      expect(stats[0]['rows'], 100);
      expect(stats[1]['rows'], 200);
      expect(stats[2]['__db_file__'], 'size');
    });

    test('safeDelete returns false on error', () async {
      api.stub('crateFfiDbDbExecuteRaw', (_) {
        throw Exception('SQL error');
      });

      final settings = SettingsService();
      const sql = 'DELETE FROM posts';
      final result = await settings.safeDelete(sql);

      expect(result, false);
    });

    test('purgeAllPosts marks all posts as deleted', () async {
      api.stub('crateFfiDbDbExecuteRaw', (_) {});

      final settings = SettingsService();
      await settings.purgeAllPosts();

      final inv = api.callsOf('crateFfiDbDbExecuteRaw').single;
      final sql = api.namedArg(inv, 'sql') as String;
      expect(
        sql,
        'UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0',
      );
    });
  });
}
