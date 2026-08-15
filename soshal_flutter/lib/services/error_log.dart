import 'dart:async';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

/// Appends a runtime error to `<app documents>/soshal-error.log` so failures
/// shown only in the debug banner / SnackBar remain readable and copyable.
/// Never throws: logging must not crash the app.
Future<void> logRuntimeError(Object error, [StackTrace? stack]) async {
  final buffer = StringBuffer()
    ..writeln('[${DateTime.now().toIso8601String()}] ERROR')
    ..writeln('$error');
  if (stack != null) buffer.writeln('$stack');
  buffer.writeln('---');
  try {
    final dir = await getApplicationDocumentsDirectory();
    await dir.create(recursive: true);
    final file = File('${dir.path}/soshal-error.log');
    await file.writeAsString(buffer.toString(), mode: FileMode.append);
  } catch (_) {}
}

/// Standardized error store for services: surfaces to UI via [lastError]
/// AND logs to terminal + soshal-error.log, so errors never vanish.
mixin LastErrorMixin {
  String? _lastError;
  String? get lastError => _lastError;

  void setLastError(Object error, [StackTrace? stack]) {
    _lastError = error.toString();
    debugPrint('SVC ERROR: $error');
    logRuntimeError('svc: $error', stack);
  }

  void clearLastError() {
    _lastError = null;
  }
}

/// [ChangeNotifier.notifyListeners] deferred to the next microtask.
/// Bridge fns are `#[frb(sync)]`, so `await service.fetchX()` runs
/// synchronously: a fetch reachable from `initState` would call
/// [notifyListeners] mid-build and trip the "setState() or
/// markNeedsBuild() called during build" assertion. Microtasks drain
/// only after the frame's synchronous pipeline completes, so this is
/// safe from build/layout/paint.
mixin DeferredNotify on ChangeNotifier {
  void notifyDeferred() => scheduleMicrotask(notifyListeners);
}
