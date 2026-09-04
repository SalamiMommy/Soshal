// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

import 'error_log.dart';
import 'ffi_bridge.dart';

/// Backup operations: export/restore the SQLite store.
class BackupService extends ChangeNotifier with LastErrorMixin {
  /// Get the backup database path (beside the live DB).
  Future<String> getBackupPath() async {
    final dbPath = await FfiBridge.getDbPath();
    return '${dbPath.substring(0, dbPath.lastIndexOf('/'))}/soshal_backup.db';
  }

  /// Count rows in a local table.
  int dbCount(String table) =>
      RustLib.instance.api.crateFfiDbDbCount(table: table);

  /// Export the SQLite store to [path].
  Future<String> backup(String path) async {
    try {
      final result = RustLib.instance.api.crateFfiDbDbBackup(backupPath: path);
      clearLastError();
      notifyListeners();
      return result;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Restore the SQLite store from [path].
  Future<String> restore(String path) async {
    try {
      final result = RustLib.instance.api.crateFfiDbDbRestore(backupPath: path);
      clearLastError();
      notifyListeners();
      return result;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}
