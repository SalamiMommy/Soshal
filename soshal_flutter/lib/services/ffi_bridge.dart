// ignore_for_file: invalid_use_of_internal_member
import 'dart:io' show Platform;
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    as frb;
import 'package:soshal_flutter/frb_generated.dart';
import '../ffi/db.dart' as db_ffi;

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
    final existing = _initFuture;
    if (existing != null) return existing;
    final future = _doInit();
    _initFuture = future;
    future.then(
      (_) {},
      onError: (_) {
        _initFuture = null;
      },
    );
    return future;
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

  /// Initialize the database with automatic migration handling.
  /// This checks the current schema version and forces a migration if
  /// the database is out of date (handles cases where the schema version
  /// is stuck at an older version due to failed migrations).
  static Future<String> initDatabase() async {
    final dbPath = await getDbPath();
    try {
      final result = RustLib.instance.api.crateFfiDbDbInit(dbPath: dbPath);
      // Check if the schema version is current
      try {
        final currentVersion = db_ffi.dbSchemaVersion();
        const expectedVersion = 1; // Must match db-core SCHEMA_VERSION
        debugPrint('Database schema version: $currentVersion, expected: $expectedVersion');
        
        if (currentVersion > expectedVersion) {
          debugPrint('Database schema version $currentVersion is ahead of expected $expectedVersion. This may cause compatibility issues. Forcing migration to reset to current schema...');
          final migrateResult = db_ffi.dbForceMigrate();
          debugPrint('Migration result: $migrateResult');
          // Verify the migration succeeded
          final newVersion = db_ffi.dbSchemaVersion();
          debugPrint('Database schema version after migration: $newVersion');
          if (newVersion != expectedVersion) {
            throw Exception('Migration failed to reset schema version to $expectedVersion');
          }
        } else if (currentVersion < expectedVersion) {
          debugPrint('Database schema version $currentVersion is behind expected $expectedVersion, forcing migration...');
          final migrateResult = db_ffi.dbForceMigrate();
          debugPrint('Migration result: $migrateResult');
          // Verify the migration succeeded
          final newVersion = db_ffi.dbSchemaVersion();
          debugPrint('Database schema version after migration: $newVersion');
          if (newVersion < expectedVersion) {
            throw Exception('Migration failed to update schema version to $expectedVersion');
          }
        }
      } catch (e) {
        debugPrint('Error checking schema version: $e');
        // Continue anyway - the init may have succeeded
      }
      return result;
    } catch (e) {
      debugPrint('Database init failed: $e');
      rethrow;
    }
  }
}
