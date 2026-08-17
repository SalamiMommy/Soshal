/// Off-thread decode helper.
///
/// Runs [fn] via [compute] on a background isolate in production, but falls
/// back to the main thread under `flutter test`: the test binding's FakeAsync
/// zone is captured by the closure sent to the isolate, which makes it
/// unsendable ("object is unsendable - _CustomZone"). `FLUTTER_TEST` is
/// defined by the test runner (dart:io `bool.fromEnvironment`).
library;

import 'dart:io';

import 'package:flutter/foundation.dart';

/// Call [fn] off the UI thread; on the main thread in widget tests.
///
/// Under `flutter test` the result is a [SynchronousFuture], so awaiting it
/// resumes the caller synchronously — mirroring the pre-isolate decode path
/// that the service tests were written against (their `notifyDeferred`
/// microtask must beat the test's own continuation).
Future<T> runOffThread<T>(T Function() fn) {
  if (Platform.environment['FLUTTER_TEST'] == 'true' ||
      const bool.fromEnvironment('FLUTTER_TEST')) {
    return SynchronousFuture<T>(fn());
  }
  return compute<void, T>((_) => fn(), null);
}
