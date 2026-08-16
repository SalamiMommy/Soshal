// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/backup_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-backup');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('BackupService', () {
    test('getBackupPath sits beside the live db path', () async {
      final service = BackupService();
      final path = await service.getBackupPath();

      expect(path, 'test-backup/soshal_backup.db');
      expect(api.calls, isEmpty, reason: 'no FFI involved');
    });

    test('dbCount forwards table name and returns count', () {
      api.stub('crateFfiDbDbCount', (_) => 42);

      final service = BackupService();
      final count = service.dbCount('posts');

      expect(count, 42);
      final inv = api.callsOf('crateFfiDbDbCount').single;
      expect(api.namedArg(inv, 'table'), 'posts');
    });

    test('dbCount forwards empty table name', () {
      api.stub('crateFfiDbDbCount', (_) => 0);

      final service = BackupService();
      expect(service.dbCount(''), 0);
      final inv = api.callsOf('crateFfiDbDbCount').single;
      expect(api.namedArg(inv, 'table'), '');
    });

    test('dbCount propagates backend error', () {
      api.stub('crateFfiDbDbCount', (_) {
        throw Exception('no such table');
      });

      final service = BackupService();
      expect(() => service.dbCount('missing'), throwsA(isA<Exception>()));
    });

    test('backup forwards path and returns confirmation', () async {
      api.stubString('crateFfiDbDbBackup', 'backed up');

      final service = BackupService();
      final result = await service.backup('/tmp/soshal_backup.db');

      expect(result, 'backed up');
      final inv = api.callsOf('crateFfiDbDbBackup').single;
      expect(api.namedArg(inv, 'backupPath'), '/tmp/soshal_backup.db');
    });

    test('backup propagates backend error', () {
      api.stub('crateFfiDbDbBackup', (_) {
        throw Exception('backup failed');
      });

      final service = BackupService();
      expect(service.backup('/tmp/x.db'), throwsA(isA<Exception>()));
    });

    test('restore forwards path and returns confirmation', () async {
      api.stubString('crateFfiDbDbRestore', 'restored');

      final service = BackupService();
      final result = await service.restore('/tmp/soshal_backup.db');

      expect(result, 'restored');
      final inv = api.callsOf('crateFfiDbDbRestore').single;
      expect(api.namedArg(inv, 'backupPath'), '/tmp/soshal_backup.db');
    });

    test('restore propagates backend error', () {
      api.stub('crateFfiDbDbRestore', (_) {
        throw Exception('restore failed');
      });

      final service = BackupService();
      expect(service.restore('/tmp/x.db'), throwsA(isA<Exception>()));
    });
  });
}