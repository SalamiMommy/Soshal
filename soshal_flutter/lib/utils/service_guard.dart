import 'dart:async';

import 'package:flutter/foundation.dart';

import '../services/error_log.dart';

/// Encapsulates the standardized service method skeleton:
///
///     try {
///       final r = ...;
///       clearLastError();
///       notifyListeners();
///       return r;
///     } catch (e, st) {
///       setLastError(e, st);
///       notifyListeners();
///       rethrow;
///     }
///
/// Both sync and async bodies are supported ([FutureOr]); results are unified
/// through a single `await`. Error/notify semantics of every migrated call
/// stay identical to the hand-written body:
///
/// - On the sync path the clear/error state is updated immediately, but the
///   notify is deferred to a microtask, so a `guard` invoked during the build
///   phase (e.g. a service fetch kicked from `initState`) cannot call
///   `setState()`/`markNeedsBuild()` while the widget tree is mid-build.
/// - [onNotify] fires instead of [notifyListeners] when the service mixes in
///   [DeferredNotify] (pass `onNotify: notifyDeferred`).
/// - [clearOnSuccess] / [notifyOnSuccess] disable the success-time
///   clear+notify for methods that only touch error state in the catch.
/// - [notifyOnError] disables the catch-time notify for methods whose
///   notification happens elsewhere (e.g. a `finally`).
mixin ServiceGuard on ChangeNotifier, LastErrorMixin {
  /// Runs [body] and returns a [Future] that matches the successful result or
  /// the rethrown error. When [body] completes synchronously (a non-`Future`
  /// return value, or a synchronous throw), the clear/error steps run in the
  /// same synchronous turn but the notify is deferred to a microtask — the
  /// hand-written async skeleton's equivalent deferred notify, safe to call
  /// from `initState`. Future-returning bodies follow the deferred path,
  /// matching bodies that awaited a real future.
  Future<T> guard<T>(
    FutureOr<T> Function() body, {
    void Function()? onNotify,
    bool clearOnSuccess = true,
    bool notifyOnSuccess = true,
    bool notifyOnError = true,
  }) {
    final n = onNotify ?? notifyListeners;
    try {
      final r = body();
      if (r is Future<T>) {
        return r.then<T>((v) {
          if (clearOnSuccess) clearLastError();
          if (notifyOnSuccess) n();
          return v;
        }, onError: (Object e, StackTrace st) {
          setLastError(e, st);
          if (notifyOnError) n();
          return Future<T>.error(e, st);
        });
      }
      if (clearOnSuccess) clearLastError();
      if (notifyOnSuccess) scheduleMicrotask(n);
      return Future<T>.value(r);
    } catch (e, st) {
      setLastError(e, st);
      if (notifyOnError) scheduleMicrotask(n);
      return Future<T>.error(e, st);
    }
  }

  /// Sync variant of [guard] for methods that return a plain value (no
  /// `Future`) while still running the error/notify skeleton.
  T guardSync<T>(
    T Function() body, {
    void Function()? onNotify,
    bool clearOnSuccess = true,
    bool notifyOnSuccess = true,
    bool notifyOnError = true,
  }) {
    final n = onNotify ?? notifyListeners;
    try {
      final r = body();
      if (clearOnSuccess) clearLastError();
      if (notifyOnSuccess) n();
      return r;
    } catch (e, st) {
      setLastError(e, st);
      if (notifyOnError) n();
      Error.throwWithStackTrace(e, st);
    }
  }
}
