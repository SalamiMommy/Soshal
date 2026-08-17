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
}
