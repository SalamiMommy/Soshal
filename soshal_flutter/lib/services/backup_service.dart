// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

import 'ffi_bridge.dart';

/// Backup operations: export/restore the SQLite store.
class BackupService extends ChangeNotifier {
  /// Get the backup database path (beside the live DB).
  Future<String> getBackupPath() async {
    final dbPath = await FfiBridge.getDbPath();
    return '${dbPath.substring(0, dbPath.lastIndexOf('/'))}/soshal_backup.db';
  }

  /// Count rows in a local table.
  int dbCount(String table) =>
      RustLib.instance.api.crateFfiDbDbCount(table: table);

  /// Export the SQLite store to [path].
  Future<String> backup(String path) async =>
      RustLib.instance.api.crateFfiDbDbBackup(backupPath: path);

  /// Restore the SQLite store from [path].
  Future<String> restore(String path) async =>
      RustLib.instance.api.crateFfiDbDbRestore(backupPath: path);
}
