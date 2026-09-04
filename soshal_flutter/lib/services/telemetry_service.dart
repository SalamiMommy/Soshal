// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:soshal_flutter/frb_generated.dart';

import 'error_log.dart';
import 'error_log.dart' as error_log;

/// Flight recorder service. Owns the telemetry init + crash hooks; records
/// app lifecycle/state events into the Rust ring buffer. On an unhandled
/// exception the recorder is sealed — the transcript survives the crash.
class TelemetryService extends ChangeNotifier with LastErrorMixin {
  static const _capacityMb = 5;
  static const _appKind = 5; // RecordKind::App
  static const _stateKind = 1; // RecordKind::State

  bool _ready = false;
  bool _sealed = false;

  bool get ready => _ready;
  bool get sealed => _sealed;

  /// Must be called after FfiBridge.init().
  Future<void> init() async {
    if (_ready) return;
    try {
      final dir = await getApplicationDocumentsDirectory();
      final path = '${dir.path}/soshal-telemetry.bin';
      RustLib.instance.api.crateFfiTelemetryTelemetryInit(
        path: path,
        capacityMb: _capacityMb,
      );
      _sealed = RustLib.instance.api.crateFfiTelemetryTelemetryIsSealed();
      _ready = true;
      recordState('app-started');
      notifyListeners();
    } catch (e) {
      setLastError(e);
    }
  }

  void record(String msg) {
    if (!_ready || _sealed) return;
    try {
      RustLib.instance.api
          .crateFfiTelemetryTelemetryRecord(kind: _appKind, msg: msg);
    } catch (e) {
      debugPrint('telemetry record: $e');
    }
  }

  void recordState(String msg) {
    if (!_ready || _sealed) return;
    try {
      RustLib.instance.api
          .crateFfiTelemetryTelemetryRecord(kind: _stateKind, msg: msg);
    } catch (e) {
      debugPrint('telemetry record: $e');
    }
  }

  /// Seal + export the encrypted dump. Returns null on failure.
  Uint8List? dumpEncrypted() {
    try {
      final dump =
          RustLib.instance.api.crateFfiTelemetryTelemetryDumpEncrypted();
      _sealed = RustLib.instance.api.crateFfiTelemetryTelemetryIsSealed();
      return dump;
    } catch (e) {
      setLastError(e);
      return null;
    }
  }

  /// JSON: [[kind, ts_ms, payload], ...] for the in-app crash viewer.
  String readAllJson() {
    try {
      return RustLib.instance.api.crateFfiTelemetryTelemetryReadAllJson();
    } catch (e) {
      return '[]';
    }
  }

  /// Runtime info JSON (ring stats, kinds, sealed state).
  String infoJson() {
    try {
      return RustLib.instance.api.crateFfiTelemetryTelemetryInfoJson();
    } catch (e) {
      return '{}';
    }
  }

  void clear() {
    try {
      RustLib.instance.api.crateFfiTelemetryTelemetryClear();
      notifyListeners();
    } catch (e) {
      debugPrint('telemetry clear: $e');
    }
  }

  /// Mark the recorder with a fatal reason and seal it.
  void recordCrash(String reason) {
    if (!_ready || _sealed) return;
    try {
      RustLib.instance.api.crateFfiTelemetryTelemetryMarkCrash(reason: reason);
      _sealed = true;
      notifyListeners();
    } catch (e) {
      debugPrint('telemetry crash mark: $e');
    }
  }

  /// Install global crash hooks that mark the recorder with the failure.
  /// Seals on first unhandled error (Flutter or platform level).
  static void installCrashHooks(TelemetryService service) {
    FlutterError.onError = (details) {
      service.recordCrash('flutter: ${details.exception}');
      error_log.logRuntimeError(details.exception, details.stack);
      FlutterError.dumpErrorToConsole(details, forceReport: true);
      FlutterError.presentError(details);
    };
    PlatformDispatcher.instance.onError = (error, stack) {
      service.recordCrash('platform: $error');
      error_log.logRuntimeError(error, stack);
      if (kReleaseMode) return true;
      debugPrint('PLATFORM ERROR: $error');
      debugPrintStack(stackTrace: stack, maxFrames: 20);
      return true;
    };
  }
}
