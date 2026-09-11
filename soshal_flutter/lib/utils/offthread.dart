/// Off-thread decode helper.
///
/// Runs decoding tasks off the UI thread via [Isolate.run]. If a closure captures
/// unsendable lexical scope (e.g. [ChangeNotifier] or framework objects), it safely
/// falls back to execution in the calling isolate rather than throwing an error.
library;

import 'dart:async';
import 'dart:io';
import 'dart:isolate';

bool get _isTest =>
    Platform.environment['FLUTTER_TEST'] == 'true' ||
    const bool.fromEnvironment('FLUTTER_TEST');

/// Call [fn] off the main UI isolate when possible, falling back to in-isolate
/// execution if the closure captures unsendable state.
Future<T> runOffThread<T>(FutureOr<T> Function() fn) async {
  if (_isTest) {
    return await fn();
  }
  try {
    return await Isolate.run(fn);
  } catch (_) {
    // Fallback: execute in the calling isolate if isolate transmission fails.
    return await fn();
  }
}

/// Typed helper that passes [payload] explicitly to [parser] in an isolate,
/// guaranteeing that no lexical instance scope is captured.
Future<R> runOffThreadCompute<T, R>(R Function(T) parser, T payload) async {
  if (_isTest) {
    return parser(payload);
  }
  try {
    return await Isolate.run(() => parser(payload));
  } catch (_) {
    return parser(payload);
  }
}
