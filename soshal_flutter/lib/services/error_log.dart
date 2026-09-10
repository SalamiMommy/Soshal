import 'dart:async';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

/// Redacts secret-shaped strings from log lines: nsec keys, NWC URIs (which
/// embed the wallet secret), 64-char hex secrets (raw private keys), and
/// nostr event ids (privacy: feed content must not leak into logs).
final RegExp _nsecRe = RegExp(r'nsec1[0-9a-z]{20,}');
final RegExp _nwcRe = RegExp(r'nostr\+walletconnect://[^\s"<>]+');
final RegExp _hex64Re = RegExp(r'\b[0-9a-f]{64}\b');

String redactSensitive(String text) {
  return text
      .replaceAll(_nsecRe, 'nsec1…[redacted]')
      .replaceAll(_nwcRe, 'nostr+walletconnect://[redacted]')
      .replaceAll(_hex64Re, '[redacted]');
}

bool _chmodDone = false;

/// Appends a runtime error to `<app documents>/soshal-error.log` so failures
/// shown only in the debug banner / SnackBar remain readable and copyable.
/// Never throws: logging must not crash the app.
Future<void> logRuntimeError(Object error, [StackTrace? stack]) async {
  final buffer = StringBuffer()
    ..writeln('[${DateTime.now().toIso8601String()}] ERROR')
    ..writeln(redactSensitive('$error'));
  if (stack != null) buffer.writeln(redactSensitive('$stack'));
  buffer.writeln('---');
  try {
    final dir = await getApplicationDocumentsDirectory();
    await dir.create(recursive: true);
    final file = File('${dir.path}/soshal-error.log');
    // Restrict the log file to owner-only on POSIX once at initialization so
    // we don't fork a shell process on every error write. Best-effort.
    if (!_chmodDone) {
      _chmodDone = true;
      try {
        await Process.run('sh', [
          '-c',
          r'chmod 600 "$1"',
          'sh',
          file.path,
        ]);
      } catch (_) {}
    }
    await file.writeAsString(buffer.toString(), mode: FileMode.append);
  } catch (e) {
    debugPrint('error log write: $e');
  }
}

/// Standardized error store for services: surfaces to UI via [lastError]
/// AND logs to terminal + soshal-error.log, so errors never vanish.
mixin LastErrorMixin {
  String? _lastError;
  String? get lastError => _lastError;

  void setLastError(Object error, [StackTrace? stack]) {
    _lastError = error.toString();
    debugPrint('SVC ERROR: ${redactSensitive('$error')}');
    logRuntimeError('svc: $error', stack);
  }

  /// Clears the stored error; returns whether one was present (so callers
  /// can notify listeners only when the error banner must be removed).
  bool clearLastError() {
    if (_lastError == null) return false;
    _lastError = null;
    return true;
  }
}

/// [ChangeNotifier.notifyListeners] deferred to the next microtask.
/// Bridge fns are `#[frb(sync)]`, so `await service.fetchX()` runs
/// synchronously: a fetch reachable from `initState` would call
/// [notifyListeners] mid-build and trip the "setState() or
/// markNeedsBuild() called during build" assertion. Microtasks drain
/// only after the frame's synchronous pipeline completes, so this is
/// safe from build/layout/paint. Rapid/burst calls are coalesced into a
/// single notification per frame.
mixin DeferredNotify on ChangeNotifier {
  bool _notifyScheduled = false;
  bool _disposed = false;

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }

  void notifyDeferred() {
    if (_notifyScheduled || _disposed) return;
    _notifyScheduled = true;
    scheduleMicrotask(() {
      _notifyScheduled = false;
      if (!_disposed) {
        notifyListeners();
      }
    });
  }
}
