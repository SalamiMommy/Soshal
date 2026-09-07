/// Off-thread decode helper.
///
/// Closures passed from ChangeNotifier services capture lexical scope containing
/// listeners and Provider elements (which contain unsendable framework classes
/// like `_AsyncCompleter`). Running via [SynchronousFuture] avoids isolate
/// transmission failures while maintaining the [Future] async contract.
library;

import 'package:flutter/foundation.dart';

/// Call [fn] and return a [Future] with the result.
///
/// The result is a [SynchronousFuture], so awaiting it resumes the caller
/// synchronously without isolate serialization overhead or "unsendable object"
/// crashes when closures capture instance scopes.
Future<T> runOffThread<T>(T Function() fn) {
  try {
    return SynchronousFuture<T>(fn());
  } catch (e, st) {
    return Future<T>.error(e, st);
  }
}
