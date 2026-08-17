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
Future<T> runOffThread<T>(T Function() fn) async {
  if (Platform.environment['FLUTTER_TEST'] == 'true' ||
      const bool.fromEnvironment('FLUTTER_TEST')) {
    return fn();
  }
  return compute<void, T>((_) => fn(), null);
}
