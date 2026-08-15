// ignore_for_file: invalid_use_of_internal_member
import 'dart:io' show Platform;
import 'package:path_provider/path_provider.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    as frb;
import 'package:soshal_flutter/frb_generated.dart';

/// FFI Bridge to the Rust backend.
class FfiBridge {
  static Future<void>? _initFuture;

  /// Initialize the Rust bridge (call once at app startup).
  /// On Android/iOS the Rust cdylib ships inside the app; on desktop the
  /// `soshal_flutter_bridge` library must be loadable from the platform
  /// loader path.
  /// Idempotent under concurrent calls: all callers wait on the same
  /// initialization future, so RustLib.init() runs exactly once.
  static Future<void> init() {
    return _initFuture ??= _doInit();
  }

  static Future<void> _doInit() async {
    if (Platform.isAndroid || Platform.isLinux) {
      await RustLib.init(
        externalLibrary:
            frb.ExternalLibrary.open('libsoshal_flutter_bridge.so'),
      );
    } else if (Platform.isIOS || Platform.isMacOS) {
      await RustLib.init(
        externalLibrary: frb.ExternalLibrary.process(iKnowHowToUseIt: true),
      );
    } else if (Platform.isWindows) {
      await RustLib.init(
        externalLibrary: frb.ExternalLibrary.open('soshal_flutter_bridge.dll'),
      );
    } else {
      throw UnsupportedError('Unsupported platform');
    }
  }

  /// Get the database path.
  static Future<String> getDbPath() async {
    final dir = await getApplicationDocumentsDirectory();
    return '${dir.path}/soshal.db';
  }

  /// Get the backup database path (beside the live DB).
  static Future<String> getBackupPath() async {
    final dbPath = await getDbPath();
    return '${dbPath.substring(0, dbPath.lastIndexOf('/'))}/soshal_backup.db';
  }

  /// Count rows in a local table.
  static int dbCount(String table) =>
      RustLib.instance.api.crateFfiDbDbCount(table: table);

  /// Export the SQLite store to [path].
  static Future<String> backup(String path) async =>
      RustLib.instance.api.crateFfiDbDbBackup(backupPath: path);

  /// Restore the SQLite store from [path].
  static Future<String> restore(String path) async =>
      RustLib.instance.api.crateFfiDbDbRestore(backupPath: path);
}
