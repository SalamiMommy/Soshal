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

  static Future<void>? _dbInitFuture;

  /// Initialize the bridge AND the shared database (migrations included).
  /// Idempotent under concurrent calls, like [init]: every caller waits on
  /// the same future, so `db_init` runs exactly once and no caller can touch
  /// the DB before it is set (the splash/app-root double-init race).
  static Future<void> ensureDatabaseInitialized() {
    final existing = _dbInitFuture;
    if (existing != null) return existing;
    final future = _doDatabaseInit();
    _dbInitFuture = future;
    future.then(
      (_) {},
      onError: (_) {
        _dbInitFuture = null;
      },
    );
    return future;
  }

  static Future<void> _doDatabaseInit() async {
    await init();
    await initDatabase();
  }

  /// Get the database path.
  static Future<String> getDbPath() async {
    final dir = await getApplicationDocumentsDirectory();
    return '${dir.path}/soshal.db';
  }

  /// Initialize the database with automatic migration handling.
  /// `db_init` runs the Rust migration runner, which transactionally brings
  /// the schema to db-core SCHEMA_VERSION and short-circuits when already
  /// current. Never force-migrate from Dart: the old reset path dropped every
  /// table on a version mismatch (data loss) and a hardcoded expected version
  /// drifted from SCHEMA_VERSION.
  static Future<String> initDatabase() async {
    final dbPath = await getDbPath();
    final result = RustLib.instance.api.crateFfiDbDbInit(dbPath: dbPath);
    final currentVersion = db_ffi.dbSchemaVersion();
    final expectedVersion = db_ffi.dbExpectedSchemaVersion();
    debugPrint(
        'Database schema version: $currentVersion, expected: $expectedVersion');
    if (currentVersion > expectedVersion) {
      debugPrint(
          'WARNING: database schema $currentVersion is ahead of this build ($expectedVersion) '
          '— binary is older than the database. Downgrade unsupported; data preserved.');
    }
    return result;
  }
}
